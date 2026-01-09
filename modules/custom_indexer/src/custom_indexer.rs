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

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use anyhow::{bail, Result};

use acropolis_common::{
    messages::{CardanoMessage, Message, StateTransitionMessage},
    params::SECURITY_PARAMETER_K,
    Point,
};
use caryatid_sdk::{async_trait, Context, Module, Subscription};
use config::Config;
use futures::future::join_all;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::{
    chain_index::ChainIndex, configuration::CustomIndexerConfig, cursor_store::CursorStore,
    index_actor::IndexActor,
};

struct IndexConfig {
    index: Box<dyn ChainIndex>,
    default_start: Point,
    force_restart: bool,
}

pub struct CustomIndexer<CS: CursorStore> {
    indexes: Arc<Mutex<HashMap<String, IndexConfig>>>,
    cursor_store: Arc<CS>,
}

impl<CS: CursorStore> Clone for CustomIndexer<CS> {
    fn clone(&self) -> Self {
        Self {
            indexes: self.indexes.clone(),
            cursor_store: self.cursor_store.clone(),
        }
    }
}

impl<CS: CursorStore> CustomIndexer<CS> {
    pub fn new(cursor_store: CS) -> Self {
        Self {
            indexes: Arc::new(Mutex::new(HashMap::new())),
            cursor_store: Arc::new(cursor_store),
        }
    }

    pub async fn add_index<I: ChainIndex + 'static>(
        &self,
        index: I,
        default_start: Point,
        force_restart: bool,
    ) -> Result<()> {
        let name = index.name();
        let wrapper = IndexConfig {
            index: Box::new(index),
            default_start,
            force_restart,
        };
        let mut indexes = self.indexes.lock().await;
        if indexes.insert(name.clone(), wrapper).is_some() {
            bail!("index \"{name}\" was added twice");
        }

        Ok(())
    }

    async fn run(
        &self,
        context: Arc<Context<Message>>,
        cfg: CustomIndexerConfig,
        mut txs_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let indexes: HashMap<String, IndexConfig> = {
            let mut lock = self.indexes.lock().await;
            std::mem::take(&mut lock)
        };

        let mut cursors = self.cursor_store.load().await?;

        let mut sync_points: VecDeque<Point> = VecDeque::new();

        let mut actors = vec![];

        for (name, mut index) in indexes {
            let cursor = cursors.entry(name.clone()).or_default();
            if index.force_restart {
                index.index.reset(&index.default_start).await?;
                cursor.points.clear();
                cursor.next_tx = None;
            }
            if cursor.points.is_empty() {
                cursor.points.push_back(index.default_start);
            }
            let my_sync_points = if cursor.next_tx.is_some() {
                // This index failed to apply a TX from its tip.
                // We want to pass the point BEFORE that tip to chainsync,
                // so that the first point we get back IS that tip.
                let mut points = cursor.points.clone();
                points.pop_back();
                points
            } else {
                cursor.points.clone()
            };
            let Some(tip) = my_sync_points.back() else {
                bail!("cursor {name} has no history");
            };
            if sync_points.back().is_none_or(|p| p.slot() > tip.slot()) {
                sync_points = my_sync_points;
            }
            actors.push(IndexActor::new(
                name,
                index.index,
                cursor,
                SECURITY_PARAMETER_K,
            ));
        }

        if sync_points.is_empty() {
            warn!("no indexes configured, nothing to do");
            return Ok(());
        }

        if !cfg.sync_mode().is_mithril() {
            // TODO: pass multiple points
            utils::change_sync_point(
                sync_points.back().unwrap().clone(),
                context,
                &cfg.sync_command_publisher_topic,
            )
            .await?;
        } else {
            utils::start_mithril(
                sync_points.back().unwrap().clone(),
                context,
                &cfg.sync_command_publisher_topic,
            )
            .await?;
        }

        loop {
            let (_, message) = txs_subscription.read().await?;
            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) => {
                    let block = Arc::new(block.clone());
                    let txs: Vec<Arc<[u8]>> =
                        txs_msg.txs.iter().map(|tx| Arc::<[u8]>::from(tx.as_slice())).collect();
                    join_all(actors.iter_mut().map(|a| a.apply_txs(block.clone(), &txs))).await;
                    // update cursors
                    for actor in actors.iter_mut() {
                        let cursor = cursors.get_mut(&actor.name).unwrap();
                        actor.update_cursor(cursor);
                    }
                    self.cursor_store.save(&cursors).await?;
                }
                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(point)),
                )) => {
                    join_all(actors.iter_mut().map(|a| a.rollback(point.clone()))).await;
                    // update cursors
                    for actor in actors.iter_mut() {
                        let cursor = cursors.get_mut(&actor.name).unwrap();
                        actor.update_cursor(cursor);
                    }
                    self.cursor_store.save(&cursors).await?;
                }
                _ => error!("Unexpected message type: {message:?}"),
            }
        }
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
        let txs_subscription = context.subscribe(&cfg.txs_subscribe_topic).await?;
        let mut genesis_complete_subscription =
            context.subscribe(&cfg.genesis_complete_topic).await?;

        let run_context = context.clone();

        let this = self.clone();
        context.run(async move {
            match genesis_complete_subscription.read().await {
                Ok((_, message)) => {
                    if !matches!(
                        message.as_ref(),
                        Message::Cardano((_, CardanoMessage::GenesisComplete(_)))
                    ) {
                        error!("unexpected message from genesis complete topic: {message:?}");
                        return;
                    }
                }
                Err(err) => {
                    error!("could not read genesis complete message: {err:#}");
                    return;
                }
            }
            match this.run(run_context, cfg, txs_subscription).await {
                Ok(()) => {
                    info!("custom-indexer has finished");
                }
                Err(e) => {
                    error!("custom-indexer has failed: {e:#}");
                }
            }
        });

        Ok(())
    }
}
