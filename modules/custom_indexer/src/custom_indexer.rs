//! Acropolis custom indexer module for Caryatid
//!
//! This module lets downstream applications register `ChainIndex` implementations
//! that react to on-chain transactions. The indexer handles cursor persistence,
//! initial sync, and dispatching decoded transactions to user provided indices.

pub mod chain_index;
mod configuration;
pub mod cursor_store;
mod index_actor;
mod utils;

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, Mutex};

use anyhow::{bail, Result};
use config::Config;
use tracing::{error, warn};

use caryatid_sdk::{async_trait, Context, Module};

use acropolis_common::{
    configuration::StartupMethod,
    messages::{CardanoMessage, Message, StateTransitionMessage},
    Point,
};

use crate::{
    chain_index::ChainIndex,
    configuration::CustomIndexerConfig,
    cursor_store::{CursorEntry, CursorStore},
    index_actor::{index_actor, IndexCommand, IndexResult},
    utils::{change_sync_point, send_rollback_to_indexers, send_txs_to_indexers},
};

type IndexSenders = HashMap<String, mpsc::Sender<IndexCommand>>;
type SharedSenders = Arc<Mutex<IndexSenders>>;
type IndexResponse = (
    String,
    Result<IndexResult, tokio::sync::oneshot::error::RecvError>,
);

struct IndexWrapper {
    index: Box<dyn ChainIndex>,
    tip: Point,
    default_start: Point,
    halted: bool,
}

pub struct CustomIndexer<CS: CursorStore> {
    senders: SharedSenders,
    cursor_store: Arc<CS>,
}

impl<CS: CursorStore> CustomIndexer<CS> {
    pub fn new(cursor_store: CS) -> Self {
        Self {
            senders: Arc::new(Mutex::new(HashMap::new())),
            cursor_store: Arc::new(cursor_store),
        }
    }

    pub async fn add_index<I: ChainIndex + 'static>(
        &self,
        mut index: I,
        default_start: Point,
        force_restart: bool,
    ) -> Result<()> {
        let name = index.name();

        let mut indexes = self.senders.lock().await;
        if indexes.contains_key(&name) {
            warn!("CustomIndexer: index '{name}' already exists, skipping add_index");
            return Ok(());
        }

        let mut cursors = self.cursor_store.load().await?;

        let mut entry = cursors.get(&name).cloned().unwrap_or(CursorEntry {
            tip: default_start.clone(),
            halted: false,
        });

        if force_restart || entry.halted {
            index.reset(&default_start).await?;
            entry.tip = default_start.clone();
            entry.halted = true;
        }

        cursors.insert(name.clone(), entry.clone());
        self.cursor_store.save(&cursors).await?;

        let wrapper = IndexWrapper {
            index: Box::new(index),
            tip: entry.tip.clone(),
            default_start,
            halted: false,
        };

        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(index_actor(wrapper, rx));
        indexes.insert(name.clone(), tx);

        Ok(())
    }

    async fn compute_start_point(&self) -> Result<Point> {
        let saved_tips = self.cursor_store.load().await?;
        let index_names: Vec<String> = {
            let senders = self.senders.lock().await;
            senders.keys().cloned().collect()
        };

        let mut min_point = None;
        for index in index_names {
            let index_entry = saved_tips
                .get(&index)
                .unwrap_or(&CursorEntry {
                    tip: Point::Origin,
                    halted: false,
                })
                .clone();
            min_point = match min_point {
                None => Some(index_entry.tip),
                Some(current) => {
                    if index_entry.tip.slot() < current.slot() {
                        Some(index_entry.tip)
                    } else {
                        Some(current)
                    }
                }
            };
        }
        Ok(min_point.unwrap_or(Point::Origin))
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
        let mut txs_subscription = context.subscribe(&cfg.txs_subscribe_topic).await?;
        let mut genesis_complete_subscription =
            context.subscribe(&cfg.genesis_complete_topic).await?;

        let run_context = context.clone();

        let senders = Arc::clone(&self.senders);
        let cursor_store = Arc::clone(&self.cursor_store);

        // Get the lowest tip from added indexes to determine where chain sync should begin
        let start_point = self.compute_start_point().await?;

        context.run(async move {
            // Wait for genesis bootstrapping then publish initial sync point if not using mithril
            let (_, message) = genesis_complete_subscription.read().await?;
            match message.as_ref() {
                Message::Cardano((_, CardanoMessage::GenesisComplete(_))) => { }
                msg => bail!("Unexpected message in genesis completion topic: {msg:?}"),
            }

            if cfg.startup_method() != StartupMethod::Mithril {
                change_sync_point(start_point, run_context.clone(), &cfg.sync_command_publisher_topic).await?;
            }

            // Forward received txs and rollback notifications to index handlers
            loop {
                match txs_subscription.read().await {
                    Ok((_, message)) => {
                        match message.as_ref() {
                            Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) => {
                                let txs: Vec<Arc<[u8]>> = txs_msg
                                    .txs
                                    .iter()
                                    .map(|tx| Arc::<[u8]>::from(tx.as_slice()))
                                    .collect();

                                // Send txs to all index tasks
                                let responses = send_txs_to_indexers(&senders, block, &txs).await;

                                // Get responses with new tips and any halts that occured
                                let new_entries = process_tx_responses(responses, block.slot).await;

                                // Save the new entries to the cursor store
                                cursor_store.save(&new_entries).await?;

                            }

                            Message::Cardano((
                                _,
                                CardanoMessage::StateTransition(StateTransitionMessage::Rollback(
                                    point,
                                )),
                            )) => {
                                // Inform indexes of a rollback
                                let responses = send_rollback_to_indexers(&senders, point).await;

                                // Get responses with new tips and any indexes which failed to rollback and could not be reset
                                let (new_tips, to_remove) = process_rollback_responses(
                                    responses,
                                    run_context.clone(),
                                    &cfg.sync_command_publisher_topic,
                                ).await?;

                                // Save the new entries to the cursor store
                                cursor_store.save(&new_tips).await?;

                                // Remove any indexes which were unable to rollback or reset successfully
                                if !to_remove.is_empty() {
                                    let mut guard = senders.lock().await;

                                    for name in &to_remove {
                                        if guard.remove(name).is_some() {
                                            warn!("Removed sender for '{name}' due to fatal reset error");
                                        } else {
                                            warn!("Tried to remove sender for '{name}' but it wasn't present");
                                        }
                                    }
                                }
                            }
                            _ => error!("Unexpected message type: {message:?}"),
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

async fn process_tx_responses<F: futures::Future<Output = IndexResponse> + Send>(
    mut results: FuturesUnordered<F>,
    block_slot: u64,
) -> HashMap<String, CursorEntry> {
    let mut new_tips = HashMap::new();

    while let Some((name, result)) = results.next().await {
        match result {
            Ok(IndexResult::Success { entry }) => {
                new_tips.insert(name, entry);
            }
            Ok(IndexResult::HandleError { entry, reason }) => {
                error!(
                    "Failed to handle tx at slot {} for index '{}': {}",
                    block_slot, name, reason
                );
                new_tips.insert(name, entry);
            }
            Ok(IndexResult::Halted) => {
                warn!("Index '{}' is halted", name);
            }
            Err(_) => {
                error!("Actor for index '{}' dropped unexpectedly", name);
            }
            _ => error!("Unexpected index result type: {result:?}"),
        }
    }

    new_tips
}

async fn process_rollback_responses<F: futures::Future<Output = IndexResponse> + Send>(
    mut results: FuturesUnordered<F>,
    run_context: Arc<Context<Message>>,
    sync_topic: &str,
) -> Result<(HashMap<String, CursorEntry>, Vec<String>)> {
    let mut new_tips = HashMap::new();
    let mut to_remove = Vec::new();

    while let Some((name, result)) = results.next().await {
        match result {
            Ok(IndexResult::Success { entry }) => {
                new_tips.insert(name, entry);
            }
            Ok(IndexResult::Reset { entry }) => {
                // Update tip and publish sync command to start fetching blocks from this point
                new_tips.insert(name.clone(), entry.clone());
                change_sync_point(entry.tip, run_context.clone(), &sync_topic.to_string()).await?;
            }
            Ok(IndexResult::FatalResetError { entry, reason }) => {
                // Update tip and add index for removal from senders list
                new_tips.insert(name.clone(), entry);
                to_remove.push(name.clone());
                error!("{name} failed to reset, halting and retrying next run: {reason}");
            }
            Err(_) => {
                error!("Actor for {name} index dropped");
            }
            _ => error!("Unexpected index result type: {result:?}"),
        }
    }

    Ok((new_tips, to_remove))
}
