//! Acropolis custom indexer module for Caryatid
//!
//! This module lets downstream applications register `ChainIndex` implementations
//! that react to on-chain transactions. The indexer handles cursor persistence,
//! initial sync, and dispatching decoded transactions to user provided indices.

pub mod chain_index;
mod configuration;
pub mod cursor_store;

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use anyhow::Result;
use config::Config;
use tracing::{error, info, warn};

use caryatid_sdk::{async_trait, Context, Module};
use pallas::ledger::traverse::MultiEraTx;

use acropolis_common::{
    commands::chain_sync::ChainSyncCommand,
    messages::{CardanoMessage, Command, Message, StateTransitionMessage},
    Point,
};

use crate::{
    chain_index::ChainIndex, configuration::CustomIndexerConfig, cursor_store::CursorStore,
};

struct IndexWrapper {
    name: String,
    index: Box<dyn ChainIndex>,
    tip: Point,
    halted: bool,
    force_restart: bool,
}

pub struct CustomIndexer<CS: CursorStore> {
    indexes: Arc<Mutex<HashMap<String, Arc<Mutex<IndexWrapper>>>>>,
    cursor_store: Arc<Mutex<CS>>,
    halted: Arc<AtomicBool>,
}

impl<CS: CursorStore> CustomIndexer<CS> {
    pub fn new(cursor_store: CS) -> Self {
        Self {
            indexes: Arc::new(Mutex::new(HashMap::new())),
            cursor_store: Arc::new(Mutex::new(cursor_store)),
            halted: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn add_index<I: ChainIndex + 'static>(
        &self,
        index: I,
        default_start: Point,
        force_restart: bool,
    ) {
        let name = index.name();

        let mut map = self.indexes.lock().await;
        if map.contains_key(&name) {
            warn!("CustomIndexer: index '{name}' already exists, skipping add_index");
            return;
        }

        map.insert(
            name.clone(),
            Arc::new(Mutex::new(IndexWrapper {
                name,
                index: Box::new(index),
                tip: default_start,
                halted: false,
                force_restart,
            })),
        );
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

        let indexes = Arc::clone(&self.indexes);
        let cursor_store = Arc::clone(&self.cursor_store);
        let halted = Arc::clone(&self.halted);

        // Retrieve saved index tips from cursor store
        let saved = cursor_store.lock().await.load().await?;

        // Compute start_point by collecting wrapper arcs, then locking each wrapper
        let start_point = {
            let mut min_point = None;

            let wrappers = {
                let map = self.indexes.lock().await;
                map.values().cloned().collect::<Vec<_>>()
            };

            for wrapper_arc in wrappers {
                let mut wrapper = wrapper_arc.lock().await;

                if !wrapper.force_restart {
                    if let Some(saved_point) = saved.get(&wrapper.name) {
                        wrapper.tip = saved_point.clone();
                    }
                }

                let tip = wrapper.tip.clone();

                min_point = match min_point {
                    None => Some(tip),
                    Some(ref current) => {
                        if tip.slot_or_default() < current.slot_or_default() {
                            Some(tip)
                        } else {
                            Some(current.clone())
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
                            Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) => {
                                // Do not continue processing if awaiting a rollback due to failed tx decode
                                if halted.load(Ordering::SeqCst) {
                                    continue;
                                }

                                // Decode all txs and set global halt flag if any decode failures
                                let mut decode_failed = false;
                                let mut decoded = Vec::with_capacity(txs_msg.txs.len());
                                for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
                                    match MultiEraTx::decode(raw_tx) {
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
                                    }
                                }

                                if decode_failed {
                                    continue;
                                }

                                // Call handle_onchain_tx on all non-halted indexes.
                                // We collect Arc clones of each wrapper first so we do not hold
                                // the map lock while awaiting on index handlers.
                                let wrappers = {
                                    let map = indexes.lock().await;
                                    map.values().cloned().collect::<Vec<_>>()
                                };

                                let block_point = Point::Specific {
                                    hash: block.hash,
                                    slot: block.slot,
                                };

                                process_onchain_for_all(wrappers, block_point, block, &decoded).await;

                                // Save tips
                                if !halted.load(Ordering::SeqCst) {
                                    let tips = {
                                        let entries = {
                                            let map = indexes.lock().await;
                                            map.iter()
                                                .map(|(k, v)| (k.clone(), v.clone()))
                                                .collect::<Vec<_>>()
                                        };

                                        let mut out = HashMap::new();
                                        for (k, w_arc) in entries {
                                            let w = w_arc.lock().await;
                                            out.insert(k, w.tip.clone());
                                        }
                                        out
                                    };

                                    cursor_store.lock().await.save(&tips).await?;
                                }
                            }

                            Message::Cardano((
                                _,
                                CardanoMessage::StateTransition(StateTransitionMessage::Rollback(
                                    point,
                                )),
                            )) => {
                                // Collect wrapper arcs then process each without holding map lock
                                let wrappers = {
                                    let map = indexes.lock().await;
                                    map.values().cloned().collect::<Vec<_>>()
                                };

                                for wrapper_arc in wrappers.iter() {
                                    let mut wrapper = wrapper_arc.lock().await;
                                    if let Err(e) = wrapper.index.handle_rollback(point).await {
                                        error!(
                                            "Rollback error in index '{}': {e:#}",
                                            wrapper.name
                                        );
                                    } else {
                                        wrapper.tip = point.clone();
                                    }
                                }

                                // Rollback tips and save
                                let tips = {
                                    let map = indexes.lock().await;
                                    let entries = map.iter()
                                        .map(|(k, v)| (k.clone(), v.clone()))
                                        .collect::<Vec<_>>();

                                    let mut out = HashMap::new();
                                    for (k, w_arc) in entries {
                                        let w = w_arc.lock().await;
                                        out.insert(k, w.tip.clone());
                                    }
                                    out
                                };

                                cursor_store.lock().await.save(&tips).await?;

                                // Remove halt
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

async fn process_onchain_for_all(
    wrappers: Vec<Arc<Mutex<IndexWrapper>>>,
    at: Point,
    block: &acropolis_common::BlockInfo,
    decoded: &[MultiEraTx<'_>],
) {
    let mut futs = FuturesUnordered::new();

    for wrapper_arc in wrappers {
        let at = at.clone();
        let block = block.clone();

        futs.push(async move {
            let mut wrapper = wrapper_arc.lock().await;

            if wrapper.halted {
                return;
            }

            if wrapper.tip.slot_or_default() > at.slot_or_default() {
                return;
            }

            for tx in decoded {
                if let Err(e) = wrapper.index.handle_onchain_tx(&block, tx).await {
                    error!(
                        "index '{}' failed on block {}: {e:#}",
                        wrapper.name, block.number
                    );
                    wrapper.halted = true;
                    return;
                }
            }

            // only update tip if all txs succeeded
            wrapper.tip = at;
        });
    }

    while futs.next().await.is_some() {}
}
