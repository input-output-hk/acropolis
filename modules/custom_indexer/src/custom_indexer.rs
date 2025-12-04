//! Acropolis custom indexer module for Caryatid
//!
//! This module lets downstream applications register `ChainIndex` implementations
//! that react to on-chain transactions. The indexer handles cursor persistence,
//! initial sync, and dispatching decoded transactions to user provided indices.

pub mod chain_index;
mod configuration;
pub mod cursor_store;
mod index_actor;

use acropolis_common::BlockInfo;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, oneshot, Mutex};

use anyhow::Result;
use config::Config;
use tracing::{error, info, warn};

use caryatid_sdk::{async_trait, Context, Module};
use pallas::ledger::traverse::{MultiEraTx};

use acropolis_common::{
    commands::chain_sync::ChainSyncCommand,
    messages::{CardanoMessage, Command, Message, StateTransitionMessage},
    Point,
};

use crate::{
    chain_index::ChainIndex, configuration::CustomIndexerConfig, cursor_store::CursorStore,
    index_actor::index_actor,
};

enum IndexCommand {
    ApplyTxs {
        block: BlockInfo,
        txs: Vec<MultiEraTx<'static>>,
        response_tx: oneshot::Sender<IndexResult>,
    },
    Rollback {
        point: Point,
        response_tx: oneshot::Sender<IndexResult>,
    },
}

enum IndexResult {
    Success { tip: Point },
    Failed { reason: String },
}

type IndexSenders = HashMap<String, mpsc::Sender<IndexCommand>>;
type SharedSenders = Arc<Mutex<IndexSenders>>;

struct IndexWrapper {
    name: String,
    index: Box<dyn ChainIndex>,
    tip: Point,
    halted: bool,
}

pub struct CustomIndexer<CS: CursorStore> {
    senders: SharedSenders,
    cursor_store: Arc<CS>,
    halted: Arc<AtomicBool>,
}

impl<CS: CursorStore> CustomIndexer<CS> {
    pub fn new(cursor_store: CS) -> Self {
        Self {
            senders: Arc::new(Mutex::new(HashMap::new())),
            cursor_store: Arc::new(cursor_store),
            halted: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn add_index<I: ChainIndex + 'static>(
        &self,
        index: I,
        default_start: Point,
        force_restart: bool,
    ) -> Result<()> {
        let name = index.name();

        let mut indexes = self.senders.lock().await;
        if indexes.contains_key(&name) {
            warn!("CustomIndexer: index '{name}' already exists, skipping add_index");
            return Ok(());
        }

        let mut tips = self.cursor_store.load().await?;

        let tip = if force_restart {
            tips.insert(name.clone(), default_start.clone());
            self.cursor_store.save(&tips).await?;
            default_start.clone()
        } else {
            tips.get(&name).cloned().unwrap_or(default_start.clone())
        };

        let wrapper = IndexWrapper {
            name: name.clone(),
            index: Box::new(index),
            tip,
            halted: false,
        };

        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(index_actor(wrapper, rx));
        indexes.insert(name.clone(), tx);

        Ok(())
    }
}

#[async_trait]
impl<CS> Module<Message> for CustomIndexer<CS>
where
    CS: CursorStore,
{
    fn get_name(&self) -> &'static str {
        "custom-indexer"
    }

    fn get_description(&self) -> &'static str {
        "Single external chain indexer module"
    }

    async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = CustomIndexerConfig::try_load(&config)?;
        let mut subscription = context.subscribe(&cfg.txs_subscribe_topic).await?;
        let run_context = context.clone();

        let senders = Arc::clone(&self.senders);
        let cursor_store = Arc::clone(&self.cursor_store);
        let halted = Arc::clone(&self.halted);

        // Retrieve saved index tips from cursor store
        let start_point = {
            let saved_tips = cursor_store.load().await?;
            let index_names: Vec<String> = {
                let senders = self.senders.lock().await;
                senders.keys().cloned().collect()
            };

            let mut min_point = None;
            for index in index_names {
                let index_tip = saved_tips.get(&index).unwrap_or(&Point::Origin).clone();
                min_point = match min_point {
                    None => Some(index_tip),
                    Some(current) => {
                        if index_tip.slot_or_default() < current.slot_or_default() {
                            Some(index_tip)
                        } else {
                            Some(current)
                        }
                    }
                };
            }
            min_point.unwrap_or(Point::Origin)
        };

        context.run(async move {
            // Publish initial sync point
            let msg = Message::Command(Command::ChainSync(ChainSyncCommand::FindIntersect(
                start_point,
            )));
            run_context.publish(&cfg.sync_command_publisher_topic, Arc::new(msg)).await?;
            info!(
                "Publishing initial sync command on {}",
                cfg.sync_command_publisher_topic
            );

            // Forward received txs and rollback notifications to index handlers
            loop {
                match subscription.read().await {
                    Ok((_, message)) => {
                        match message.as_ref() {
                            Message::Cardano((block_ref, CardanoMessage::ReceivedTxs(txs_msg_ref))) => {
                                let block = block_ref.clone();

                                // Do not continue processing if awaiting a rollback due to failed tx decode
                                if halted.load(Ordering::SeqCst) {
                                    continue;
                                }

                                let mut decode_failed = false;
                                let mut decoded = Vec::with_capacity(txs_msg_ref.txs.len());
                                for (tx_index, raw_tx) in txs_msg_ref.txs.iter().enumerate() {
                                    let raw = raw_tx.clone();
                                    match MultiEraTx::decode(&raw) {
                                        Ok(tx) => decoded.push(tx),
                                        Err(e) => {
                                            warn!(
                                                "Failed to decode tx {} in block {}, halting: {e:#}",
                                                tx_index, block.number
                                            );
                                            halted.store(true, Ordering::SeqCst);
                                            decode_failed = true;
                                            break;
                                        }
                                    };  
                                }


                                if decode_failed {
                                    continue;
                                }

                                let senders_snapshot: Vec<_> = {
                                    let map = senders.lock().await;
                                    map.iter().map(|(n, tx)| (n.clone(), tx.clone())).collect()
                                };

                                let mut results = FuturesUnordered::new();
                                for (name, sender) in senders_snapshot {
                                    let (response_tx, response_rx) = oneshot::channel();

                                     let cmd = IndexCommand::ApplyTxs {
                                        block: block.clone(),
                                        txs: decoded.clone(),
                                        response_tx,
                                    };

                                    let _ = sender.send(cmd).await;

                                    results.push(async move { (name, response_rx.await) });
                                }

                                let mut new_tips = HashMap::new();
                                while let Some((name, result)) = results.next().await {
                                    match result {
                                        Ok(IndexResult::Success { tip }) => { new_tips.insert(name, tip); }
                                        Ok(IndexResult::Failed { reason }) => { error!("Failed to handle tx at slot {} for {name} index: {reason}", block.slot) }
                                        Err(_) => { /* actor dropped */ }
                                    }
                                }

                                // Save tips
                                cursor_store.save(&new_tips).await?;
                                
                            }

                            Message::Cardano((
                                _,
                                CardanoMessage::StateTransition(StateTransitionMessage::Rollback(
                                    point,
                                )),
                            )) => {
                                // Collect wrapper arcs first to avoid holding lock on map
                                let senders_snapshot: Vec<_> = {
                                    let map = senders.lock().await;
                                    map.iter().map(|(n, tx)| (n.clone(), tx.clone())).collect()
                                };

                                let mut results = FuturesUnordered::new();
                                for (name, sender) in senders_snapshot {
                                    let (resp_tx, resp_rx) = oneshot::channel();

                                    let cmd = IndexCommand::Rollback {
                                        point: point.clone(),
                                        response_tx: resp_tx,
                                    };

                                    let _ = sender.send(cmd).await;
                                    results.push(async move { (name, resp_rx.await) });
                                }

                                let mut new_tips = HashMap::new();

                                while let Some((name, result)) = results.next().await {
                                    match result {
                                        Ok(IndexResult::Success { tip }) => {
                                            new_tips.insert(name, tip);
                                        }
                                        Ok(IndexResult::Failed { reason }) => {
                                            error!("Rollback failed for index '{name}': {reason}");
                                            // the actor marked itself halted internally
                                        }
                                        Err(_) => {
                                            error!("Rollback actor for index '{name}' dropped");
                                        }
                                    }
                                }
                                cursor_store.save(&new_tips).await?;

                                // Remove global halt
                                halted.store(false, Ordering::SeqCst);
                            }
                            _ => (),
                        }
                    }
                    Err(e) => {
                        error!("Subscription closed: {e:#}");
                        break;
                    }
                }
            }

            Ok::<_, anyhow::Error>(())
        });

        Ok(())
    }
}

