//! Acropolis custom indexer module for Caryatid
//!
//! This module lets downstream applications register `ChainIndex` implementations
//! that react to on-chain transactions. The indexer handles cursor persistence,
//! initial sync, and dispatching decoded transactions to user provided indices.

pub mod chain_index;
mod configuration;
pub mod cursor_store;
pub mod grpc_server;
mod index_actor;
mod utils;

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use anyhow::{bail, Result};

use acropolis_common::{
    messages::{CardanoMessage, Message, StateTransitionMessage},
    params::SECURITY_PARAMETER_K,
    BlockHash, BlockInfo, Point,
};
use caryatid_sdk::{async_trait, Context, Module, Subscription};
use config::Config;
use futures::future::join_all;
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{error, info, warn};

use crate::{
    chain_index::ChainIndex, configuration::CustomIndexerConfig, cursor_store::CursorStore,
    index_actor::IndexActor,
};

const EVENT_CHANNEL_CAPACITY: usize = 256;

/// A chain event emitted by the indexer after processing a block or rollback.
#[derive(Clone, Debug)]
pub enum ChainEvent {
    RollForward { block: Arc<BlockInfo>, tx_count: u32 },
    RollBack(Point),
}

/// Stored block metadata, kept in memory for recent blocks.
#[derive(Clone, Debug)]
pub struct BlockRecord {
    pub block_number: u64,
    pub hash: BlockHash,
    pub epoch: u64,
    pub slot: u64,
    pub timestamp: u64,
    pub tx_count: u32,
}

/// In-memory block index, keyed by slot for efficient rollback.
/// Also provides hash-based lookup.
#[derive(Default)]
struct BlockIndex {
    /// slot → BlockRecord (ordered for efficient rollback truncation)
    by_slot: BTreeMap<u64, BlockRecord>,
    /// hash → slot (reverse index for hash lookups)
    hash_to_slot: HashMap<BlockHash, u64>,
}

impl BlockIndex {
    fn insert(&mut self, record: BlockRecord) {
        self.hash_to_slot.insert(record.hash, record.slot);
        self.by_slot.insert(record.slot, record);
    }

    fn rollback_to(&mut self, slot: u64) {
        let to_remove: Vec<u64> = self.by_slot.range((slot + 1)..).map(|(s, _)| *s).collect();
        for s in to_remove {
            if let Some(record) = self.by_slot.remove(&s) {
                self.hash_to_slot.remove(&record.hash);
            }
        }
    }

    fn get_by_hash(&self, hash: &BlockHash) -> Option<&BlockRecord> {
        let slot = self.hash_to_slot.get(hash)?;
        self.by_slot.get(slot)
    }
}

/// Read-only handle to the indexer's live state.
///
/// Passed to the gRPC server (or any other consumer) so it can query the
/// current tip and subscribe to chain events without owning a separate copy.
#[derive(Clone)]
pub struct IndexerHandle {
    tip: Arc<RwLock<Option<Point>>>,
    events_tx: broadcast::Sender<ChainEvent>,
    blocks: Arc<RwLock<BlockIndex>>,
}

impl IndexerHandle {
    /// Read the current chain tip.
    pub async fn tip(&self) -> Option<Point> {
        self.tip.read().await.clone()
    }

    /// Subscribe to chain events. Each caller gets its own receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<ChainEvent> {
        self.events_tx.subscribe()
    }

    /// Look up block metadata by hash.
    pub async fn get_block_by_hash(&self, hash: &BlockHash) -> Option<BlockRecord> {
        self.blocks.read().await.get_by_hash(hash).cloned()
    }
}

struct IndexConfig {
    index: Box<dyn ChainIndex>,
    default_start: Point,
    force_restart: bool,
}

pub struct CustomIndexer<CS: CursorStore> {
    indexes: Arc<Mutex<HashMap<String, IndexConfig>>>,
    cursor_store: Arc<CS>,
    tip: Arc<RwLock<Option<Point>>>,
    events_tx: broadcast::Sender<ChainEvent>,
    blocks: Arc<RwLock<BlockIndex>>,
}

impl<CS: CursorStore> Clone for CustomIndexer<CS> {
    fn clone(&self) -> Self {
        Self {
            indexes: self.indexes.clone(),
            cursor_store: self.cursor_store.clone(),
            tip: self.tip.clone(),
            events_tx: self.events_tx.clone(),
            blocks: self.blocks.clone(),
        }
    }
}

impl<CS: CursorStore> CustomIndexer<CS> {
    pub fn new(cursor_store: CS) -> Self {
        let (events_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            indexes: Arc::new(Mutex::new(HashMap::new())),
            cursor_store: Arc::new(cursor_store),
            tip: Arc::new(RwLock::new(None)),
            events_tx,
            blocks: Arc::new(RwLock::new(BlockIndex::default())),
        }
    }

    /// Return a read-only handle to the indexer's live state.
    ///
    /// Pass this to [`grpc_server::start_grpc_server`](crate::grpc_server::start_grpc_server)
    /// or any other consumer that needs the current tip and event stream.
    pub fn handle(&self) -> IndexerHandle {
        IndexerHandle {
            tip: self.tip.clone(),
            events_tx: self.events_tx.clone(),
            blocks: self.blocks.clone(),
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

                    let tx_count = txs_msg.txs.len() as u32;
                    let point = Point::Specific { hash: block.hash, slot: block.slot };
                    *self.tip.write().await = Some(point);
                    self.blocks.write().await.insert(BlockRecord {
                        block_number: block.number,
                        hash: block.hash,
                        epoch: block.epoch,
                        slot: block.slot,
                        timestamp: block.timestamp,
                        tx_count,
                    });
                    let _ = self.events_tx.send(ChainEvent::RollForward {
                        block,
                        tx_count,
                    });
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

                    *self.tip.write().await = Some(point.clone());
                    self.blocks.write().await.rollback_to(point.slot());
                    let _ = self.events_tx.send(ChainEvent::RollBack(point.clone()));
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
