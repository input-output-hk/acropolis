//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, multi-protocol client interface to the Cardano node

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{
    BlockInfo,
    BlockStatus,
    messages::{
        Message,
        BlockHeaderMessage,
        BlockBodyMessage,
    },
};
use std::sync::Arc;
use anyhow::{Result, anyhow};
use config::Config;
use tracing::{debug, info, error};
use tokio::sync::Mutex;
use pallas::{
    network::{
        facades::PeerClient,
        miniprotocols::{
            chainsync::{NextResponse, Tip},
            Point,
        },
    },
    ledger::traverse::MultiEraHeader,
};

const DEFAULT_HEADER_TOPIC: &str = "cardano.block.header";
const DEFAULT_BODY_TOPIC: &str = "cardano.block.body";
const DEFAULT_SNAPSHOT_COMPLETION_TOPIC: &str = "cardano.snapshot.complete";

const DEFAULT_NODE_ADDRESS: &str = "backbone.cardano.iog.io:3001";
const DEFAULT_MAGIC_NUMBER: u64 = 764824073;

const DEFAULT_SYNC_POINT: &str = "snapshot";

/// Upstream chain fetcher module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "upstream-chain-fetcher",
    description = "Mini-protocol chain fetcher from an upstream Cardano node"
)]
pub struct UpstreamChainFetcher;

impl UpstreamChainFetcher
{
    /// Fetch an individual block and unpack it into messages
    // TODO fetch in batches
    async fn fetch_block(context: Arc<Context<Message>>, config: Arc<Config>,
                         peer: &mut PeerClient, point: Point,
                         block_info: BlockInfo) -> Result<()> {
        let topic = config.get_string("body-topic").unwrap_or(DEFAULT_BODY_TOPIC.to_string());

        // Fetch the block body
        info!("Requesting single block {point:?}");
        let body = peer.blockfetch().fetch_single(point.clone()).await;

        match body {
            Ok(body) => {
                info!("Got block {point:?} body size {}", body.len());

                // Construct message
                let message = BlockBodyMessage {
                    block: block_info,
                    raw: body
                };

                let message_enum: Message = message.into();
                context.message_bus.publish(&topic, Arc::new(message_enum))
                    .await
                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));
            },

            Err(e) => error!("Can't fetch block at {point:?}: {e}")
        }

        Ok(())
    }

    /// ChainSync client loop - fetch headers and publish details, plus fetch each block
    async fn sync_to_point(context: Arc<Context<Message>>, config: Arc<Config>,
                           peer: Arc<Mutex<PeerClient>>,
                           point: Point) -> Result<()> {

        let topic = config.get_string("header-topic").unwrap_or(DEFAULT_HEADER_TOPIC.to_string());

        // Find intersect to given point
        let slot = point.slot_or_default();
        info!("Synchronising to slot {slot}");
        let mut my_peer = peer.lock().await;
        let (point, _) = my_peer.chainsync().find_intersect(vec![point]).await?;
        point.ok_or(anyhow!("Intersection for slot {slot} not found"))?;

        // Loop fetching messages
        let mut rolled_back = false;
        loop {
            let next = my_peer.chainsync().request_or_await_next().await?;

            match next {
                NextResponse::RollForward(h, Tip(point, _)) => {
                    debug!("RollForward, tip is {point:?}");

                    // Get Byron sub-tag if any
                    let tag = match h.byron_prefix {
                        Some((tag, _)) => Some(tag),
                        _ => None
                    };

                    // Decode header
                    let header = MultiEraHeader::decode(h.variant, tag, &h.cbor);
                    match header {
                        Ok(header) => {
                            let slot = header.slot();
                            let number = header.number();
                            let hash = header.hash().to_vec();
                            info!("Header for slot {slot} number {number}");

                            // Construct message
                            let block_info = BlockInfo {
                                status: if rolled_back 
                                            { BlockStatus::RolledBack }
                                        else 
                                            { BlockStatus::Volatile }, // TODO vary with 'k'
                                slot,
                                number,
                                hash: hash.clone(),
                            };
                            let message = BlockHeaderMessage {
                                block: block_info.clone(),
                                raw: h.cbor
                            };

                            let message_enum: Message = message.into();
                            context.message_bus.publish(&topic, Arc::new(message_enum))
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                            // Fetch and publish the block itself - note we need to
                            // reconstruct a Point from the header because the one we get
                            // in the RollForward is the *tip*, not the next read point
                            let fetch_point = Point::Specific(slot, hash);
                            Self::fetch_block(context.clone(), config.clone(),
                                              &mut *my_peer, fetch_point, block_info)
                                .await?;
                        }
                        Err(e) => error!("Bad header: {e}"),
                    }

                    rolled_back = false;
                },

                // TODO Handle RollBackward, publish sync message
                NextResponse::RollBackward(point, _) => {
                    info!("RollBackward to {point:?}");
                    rolled_back = true;
                },

                _ => debug!("Ignoring message: {next:?}")
            }
        }
    }

    /// ChainSync client loop - fetch headers and publish details, plus fetch each block
    async fn run_chain_sync(context: Arc<Context<Message>>, config: Arc<Config>,
                            peer: Arc<Mutex<PeerClient>>) -> Result<()> {
        let sync_point = config.get_string("sync-point").unwrap_or(DEFAULT_SYNC_POINT.to_string());
        let mut my_peer = peer.lock().await;

        match sync_point.as_str() {
            "tip" => {
                // Ask for origin but get the tip as well
                let (_, Tip(point, _)) = my_peer.chainsync().find_intersect(vec![Point::Origin]).await?;
                Self::sync_to_point(context, config, peer.clone(), point).await?;
            }
            "origin" => {
                Self::sync_to_point(context, config, peer.clone(), Point::Origin).await?;
            }
            "snapshot" => {
                // Subscribe to snapshotter and sync to its point
                let topic = config.get_string("snapshot-complete-topic")
                    .unwrap_or(DEFAULT_SNAPSHOT_COMPLETION_TOPIC.to_string());
                info!("Waiting for snapshot completion on {topic}");

                let peer = peer.clone();
                context.clone().message_bus.subscribe(&topic, move |message: Arc<Message>| {

                    let context = context.clone();
                    let config = config.clone();
                    let peer = peer.clone();

                    tokio::spawn(async move {
                        match message.as_ref() {
                            Message::SnapshotComplete(msg) => {
                                info!("Notified snapshot complete at slot {} block number {}",
                                    msg.last_block.slot, msg.last_block.number);
                                let point = Point::Specific(
                                    msg.last_block.slot,
                                    msg.last_block.hash.clone());

                                Self::sync_to_point(context, config, peer, point)
                                    .await
                                    .unwrap_or_else(|e| error!("Can't sync: {e}"));
                            }
                            _ => error!("Unexpected message type: {message:?}")
                        }
                    });

                    async {}
                })?;
            }
            _ => return Err(anyhow!("Sync point {sync_point} not understood"))
        };

        Ok(())
    }

    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let node_address = config.get_string("node-address")
            .unwrap_or(DEFAULT_NODE_ADDRESS.to_string());
        let magic_number: u64 = config.get::<u64>("magic-number")
            .unwrap_or(DEFAULT_MAGIC_NUMBER);

        info!("Connecting to {node_address} ({magic_number})");

        tokio::spawn(async move {
            // TODO Multiple peers
            let peer = PeerClient::connect(node_address, magic_number).await;

            match peer {
                Ok(peer) => {
                    info!("Connected");
                    Self::run_chain_sync(context, config, Arc::new(Mutex::new(peer)))
                        .await
                        .unwrap_or_else(|e| error!("Chain sync failed: {e}"));
                },
                Err(e) => error!("Failed to connect to peer: {e}")
            }
        });

        Ok(())
    }
}
