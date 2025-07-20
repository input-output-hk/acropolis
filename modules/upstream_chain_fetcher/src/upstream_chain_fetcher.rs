//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, multi-protocol client interface to the Cardano node

use acropolis_common::{
    calculations::slot_to_epoch,
    messages::{BlockBodyMessage, BlockHeaderMessage, CardanoMessage, Message},
    BlockInfo, BlockStatus, Era,
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{module, Context, Module};
use config::Config;
use pallas::{
    ledger::traverse::MultiEraHeader,
    network::{
        facades::PeerClient,
        miniprotocols::{
            chainsync::{NextResponse, Tip, HeaderContent},
            Point,
        },
    },
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

mod upstream_cache;
use upstream_cache::{UpstreamCache, UpstreamCacheRecord};

const DEFAULT_HEADER_TOPIC: &str = "cardano.block.header";
const DEFAULT_BODY_TOPIC: &str = "cardano.block.body";
const DEFAULT_SNAPSHOT_COMPLETION_TOPIC: &str = "cardano.snapshot.complete";

const DEFAULT_NODE_ADDRESS: &str = "backbone.cardano.iog.io:3001";
const DEFAULT_MAGIC_NUMBER: u64 = 764824073;

const DEFAULT_SYNC_POINT: &str = "snapshot";
const DEFAULT_CACHE_DIR: &str = "upstream-cache";

/// Upstream chain fetcher module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "upstream-chain-fetcher",
    description = "Mini-protocol chain fetcher from an upstream Cardano node"
)]
pub struct UpstreamChainFetcher;

impl UpstreamChainFetcher {
    /// Fetch an individual block and unpack it into messages
    // TODO fetch in batches
    async fn fetch_block(
        //context: Arc<Context<Message>>,
        //config: Arc<Config>,
        peer: &mut PeerClient,
        point: Point,
        block_info: &BlockInfo,
    ) -> Result<Arc<BlockBodyMessage>> {
        // Fetch the block body
        debug!("Requesting single block {point:?}");
        let body = peer.blockfetch().fetch_single(point.clone()).await;

        match body {
            Ok(body) => {
                if block_info.number % 100 == 0 {
                    info!(
                        number = block_info.number,
                        size = body.len(),
                        "Fetched block"
                    );
                }

                // Construct message
                Ok(Arc::new(BlockBodyMessage { raw: body }))
            }

            Err(e) => bail!("Can't fetch block at {point:?}: {e}"),
        }
    }

    async fn publish_message(
        context: Arc<Context<Message>>, config: Arc<Config>, record: &UpstreamCacheRecord
    ) -> Result<()> {
        let hdr_topic = config.get_string("header-topic")
            .unwrap_or(DEFAULT_HEADER_TOPIC.to_string());
        let body_topic = config.get_string("body-topic")
            .unwrap_or(DEFAULT_BODY_TOPIC.to_string());

        let header_msg = Arc::new(Message::Cardano((
             record.id.clone(), 
             CardanoMessage::BlockHeader((*record.hdr).clone())
        )));

        let body_msg = Arc::new(Message::Cardano((
             record.id.clone(),
             CardanoMessage::BlockBody((*record.body).clone())
        )));

        context.message_bus.publish(&hdr_topic, header_msg).await?;
        context.message_bus.publish(&body_topic, body_msg).await
    }

    async fn process_message(
        context: Arc<Context<Message>>,
        config: Arc<Config>,
        peer: &mut PeerClient,
        cache: &mut Option<&mut UpstreamCache>,
        rolled_back: bool,
        h: HeaderContent,
        last_epoch: &mut Option<u64>,
    ) -> Result<()> {
        // Get Byron sub-tag if any
        let tag = match h.byron_prefix {
            Some((tag, _)) => Some(tag),
            _ => None,
        };

        // Decode header
        let header = MultiEraHeader::decode(h.variant, tag, &h.cbor)?;
        let slot = header.slot();
        let number = header.number();
        let hash = header.hash().to_vec();
        debug!("Header for slot {slot} number {number}");

        let epoch = slot_to_epoch(slot);
        let new_epoch = match &last_epoch {
            Some(last_epoch) => epoch != *last_epoch,
            None => true,
        };
        *last_epoch = Some(epoch);

        if new_epoch {
            info!(epoch, number, slot, "New epoch");
        }

        // Derive era from header - not complete but enough to drive
        // MultiEraHeader::decode() again at the receiver
        // TODO do this properly once we understand the values of the 'variant'
        // byte
        let era = match header {
            MultiEraHeader::EpochBoundary(_) => return Ok(()), // Ignore EBBs
            MultiEraHeader::Byron(_) => Era::Byron,
            MultiEraHeader::ShelleyCompatible(_) => Era::Shelley,
            MultiEraHeader::BabbageCompatible(_) => Era::Babbage,
        };

        // Construct message
        let block_info = BlockInfo {
            status: if rolled_back {
                BlockStatus::RolledBack
            } else {
                BlockStatus::Volatile
            }, // TODO vary with 'k'
            slot,
            number,
            hash: hash.clone(),
            epoch,
            new_epoch,
            era,
        };

        let msg_hdr = Arc::new(BlockHeaderMessage { raw: h.cbor });

        // Fetch and publish the block itself - note we need to
        // reconstruct a Point from the header because the one we get
        // in the RollForward is the *tip*, not the next read point
        let fetch_point = Point::Specific(slot, hash);
        let msg_body = Self::fetch_block(
            //context.clone(),
            //config.clone(),
            peer,
            fetch_point,
            &block_info,
        ).await?;

        let record = UpstreamCacheRecord {
            id: block_info.clone(),
            hdr: msg_hdr.clone(),
            body: msg_body.clone()
        };

        cache.as_mut().map(|cache| cache.write_record(&record)).transpose()?;
        Self::publish_message(context.clone(), config.clone(), &record).await?;

        Ok(())
    }

    /// ChainSync client loop - fetch headers and publish details, plus fetch each block
    async fn sync_to_point(
        context: Arc<Context<Message>>,
        config: Arc<Config>,
        peer: Arc<Mutex<PeerClient>>,
        mut cache: Option<&mut UpstreamCache>,
        start: Point,
    ) -> Result<()> {
        // Find intersect to given point
        let slot = start.slot_or_default();
        info!("Synchronising to slot {slot}");
        let mut my_peer = peer.lock().await;
        let (start, _) = my_peer.chainsync().find_intersect(vec![start]).await?;
        let start = start.ok_or(anyhow!("Intersection for slot {slot} not found"))?;

        // Loop fetching messages
        let mut rolled_back = false;
        let mut response_count = 0;
        let mut last_epoch: Option<u64> = match slot {
            0 => None,                      // If we're starting from origin
            _ => Some(slot_to_epoch(slot)), // From slot of last block
        };

        loop {
            response_count += 1;
            let next = my_peer.chainsync().request_or_await_next().await?;

            match next {
                NextResponse::RollForward(h, Tip(tip_point, _)) => {
                    debug!("RollForward, tip is {tip_point:?}");

                    if let Err(e) = Self::process_message(
                        context.clone(), config.clone(), &mut my_peer, &mut cache, 
                        rolled_back, h, &mut last_epoch
                    ).await {
                        error!("Cannot process 'forward' response: {e}");
                    }

                    rolled_back = false;
                },

                // TODO The first message after sync start always comes with 'RollBackward'.
                // Here we suppress this status (since it says nothing about actual rollbacks,
                // but about our sync restart). Can there arise any problems?
                NextResponse::RollBackward(point, _) if start == point && response_count == 1 => (),

                // TODO Handle RollBackward, publish sync message
                NextResponse::RollBackward(point, _) => {
                    info!("RollBackward to {point:?}");
                    rolled_back = true;
                }

                _ => debug!("Ignoring message: {next:?}"),
            }
        }
    }

    async fn read_cache(
        context: Arc<Context<Message>>,
        config: Arc<Config>,
        cache: &mut UpstreamCache,
    ) -> Result<Option<BlockInfo>> {
        let mut last_block = None;

        cache.start_reading()?;

        while let Some(record) = cache.read_record()? {
            last_block = Some(record.id.clone());
            Self::publish_message(context.clone(), config.clone(), &record).await?;
            cache.next_record()?;
        }

        Ok(last_block)
    }

    /// ChainSync client loop - fetch headers and publish details, plus fetch each block
    async fn run_chain_sync(
        context: Arc<Context<Message>>,
        config: Arc<Config>,
        peer: Arc<Mutex<PeerClient>>,
    ) -> Result<()> {
        let sync_point = config
            .get_string("sync-point")
            .unwrap_or(DEFAULT_SYNC_POINT.to_string());

        let cache_dir = config
            .get_string("cache-dir")
            .unwrap_or(DEFAULT_CACHE_DIR.to_string());

        match sync_point.as_str() {
            "tip" => {
                // Ask for origin but get the tip as well
                let mut my_peer = peer.lock().await;
                let (_, Tip(point, _)) = my_peer
                    .chainsync()
                    .find_intersect(vec![Point::Origin])
                    .await?;
                Self::sync_to_point(context, config, peer.clone(), None, point).await?;
            }
            "origin" => {
                Self::sync_to_point(context, config, peer.clone(), None, Point::Origin).await?;
            }
            "cache" => {
                let mut upstream_cache = UpstreamCache::new(cache_dir);
                let point = match Self::read_cache(
                    context.clone(), config.clone(), &mut upstream_cache
                ).await? {
                    None => Point::Origin,
                    Some(blk) => Point::Specific(blk.slot, blk.hash),
                };

                Self::sync_to_point(
                    context, config, peer.clone(), Some(&mut upstream_cache), point
                ).await?;
            }
            "snapshot" => {
                // Subscribe to snapshotter and sync to its point
                let topic = config
                    .get_string("snapshot-complete-topic")
                    .unwrap_or(DEFAULT_SNAPSHOT_COMPLETION_TOPIC.to_string());
                info!("Waiting for snapshot completion on {topic}");

                let peer = peer.clone();
                let mut subscription = context.subscribe(&topic).await?;
                context.clone().run(async move {
                    let Ok((_, message)) = subscription.read().await else {
                        return;
                    };
                    match message.as_ref() {
                        Message::Cardano((block, CardanoMessage::SnapshotComplete)) => {
                            info!(
                                "Notified snapshot complete at slot {} block number {}",
                                block.slot, block.number
                            );
                            let point = Point::Specific(block.slot, block.hash.clone());

                            Self::sync_to_point(context, config, peer, None, point)
                                .await
                                .unwrap_or_else(|e| error!("Can't sync: {e}"));
                        }
                        _ => error!("Unexpected message type: {message:?}"),
                    }
                });
            }
            _ => return Err(anyhow!("Sync point {sync_point} not understood")),
        };

        Ok(())
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let node_address =
            config.get_string("node-address").unwrap_or(DEFAULT_NODE_ADDRESS.to_string());
        let magic_number: u64 = config.get::<u64>("magic-number").unwrap_or(DEFAULT_MAGIC_NUMBER);

        info!("Connecting to {node_address} ({magic_number})");

        context.clone().run(async move {
            // TODO Multiple peers
            let peer = PeerClient::connect(node_address, magic_number).await;

            match peer {
                Ok(peer) => {
                    info!("Connected");
                    Self::run_chain_sync(context, config, Arc::new(Mutex::new(peer)))
                        .await
                        .unwrap_or_else(|e| error!("Chain sync failed: {e}"));
                }
                Err(e) => error!("Failed to connect to peer: {e}"),
            }
        });

        Ok(())
    }
}
