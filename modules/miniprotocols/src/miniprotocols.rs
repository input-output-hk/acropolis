//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, multi-protocol client interface to the Cardano node

use caryatid_sdk::{Context, Module, module};
use acropolis_messages::{BlockHeaderMessage, BlockBodyMessage, Message};
use std::sync::Arc;
use anyhow::{Result, anyhow};
use config::Config;
use tracing::{debug, info, error};

use pallas::{
    network::{
        facades::PeerClient,
        miniprotocols::{
            chainsync::{NextResponse, Tip},
            Point,
        },
    },
    ledger::{
        traverse::MultiEraHeader,
    }
};

const DEFAULT_HEADER_TOPIC: &str = "cardano.block.header";
const DEFAULT_BODY_TOPIC: &str = "cardano.block.body";

const DEFAULT_NODE_ADDRESS: &str = "preview-node.world.dev.cardano.org:30002";
const DEFAULT_MAGIC_NUMBER: u64 = 2;

const DEFAULT_SYNC_POINT: &str = "tip";

/// Network mini-protocols module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "miniprotocols",
    description = "Mini-protocol interface to the Cardano node"
)]
pub struct Miniprotocols;

impl Miniprotocols
{
    /// Fetch an individual block and unpack it into messages
    // TODO fetch in batches
    async fn fetch_block(context: Arc<Context<Message>>, config: Arc<Config>,
                         peer: &mut PeerClient, point: Point) -> Result<()> {
        let topic = config.get_string("body-topic").unwrap_or(DEFAULT_BODY_TOPIC.to_string());

        // Fetch the block body
        let body = peer.blockfetch().fetch_single(point.clone()).await;

        match body {
            Ok(body) => {
                info!("Got block {point:?} body size {}", body.len());

                // Construct message
                let message = BlockBodyMessage {
                    slot: point.slot_or_default(),
                    raw: body
                };

                debug!("Miniprotocols sending {:?}", message);

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
    async fn run_chain_sync(context: Arc<Context<Message>>, config: Arc<Config>,
                            peer: &mut PeerClient) -> Result<()> {
        let topic = config.get_string("header-topic").unwrap_or(DEFAULT_HEADER_TOPIC.to_string());
        let sync_point = config.get_string("sync-point").unwrap_or(DEFAULT_SYNC_POINT.to_string());

        match sync_point.as_str() {
            "tip" => peer.chainsync().intersect_tip().await?,
            "origin" => peer.chainsync().intersect_origin().await?,
            _ => return Err(anyhow!("Sync point {sync_point} not understood"))
        };

        info!("Synchronising to {sync_point}");

        // Loop fetching messages
        loop {
            let next = peer.chainsync().request_or_await_next().await?;

            match next {
                NextResponse::RollForward(h, Tip(point, _)) => {
                    debug!("RollForward to {point:?}");
                    match h.byron_prefix {
                        None => {
                            let header = MultiEraHeader::decode(h.variant, None, &h.cbor);
                            match header {
                                Ok(header) => {
                                    info!("Header for slot {} number {}",
                                          header.slot(), header.number());

                                    // Construct message
                                    let message = BlockHeaderMessage {
                                        slot: header.slot(),
                                        number: header.number(),
                                        raw: h.cbor
                                    };

                                    debug!("Miniprotocols sending {:?}", message);

                                    let message_enum: Message = message.into();
                                    context.message_bus.publish(&topic, Arc::new(message_enum))
                                        .await
                                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                                    // Fetch and publish the block itself
                                    Self::fetch_block(context.clone(), config.clone(),
                                                      peer, point).await?;
                                }
                                Err(e) => error!("Bad header: {e}"),
                            }
                        },

                        // TODO Handle byron blocks
                        Some(_) => info!("Skipping a Byron block"),
                    }
                },

                // TODO Handle RollBackward, publish sync message

                _ => debug!("Ignoring message: {next:?}")
            }
        }
    }

    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let node_address = config.get_string("node_address")
            .unwrap_or(DEFAULT_NODE_ADDRESS.to_string());
        let magic_number: u64 = config.get::<u64>("magic_number")
            .unwrap_or(DEFAULT_MAGIC_NUMBER);

        info!("Connecting to {node_address} ({magic_number})");

        tokio::spawn(async move {
            // TODO Multiple peers
            let peer = PeerClient::connect(node_address, magic_number).await;

            match peer {
                Ok(mut peer) => {
                    info!("Connected");
                    Self::run_chain_sync(context, config, &mut peer)
                        .await
                        .unwrap_or_else(|e| error!("Chain sync failed: {e}"));
                },
                Err(e) => error!("Failed to connect to peer: {e}")
            }
        });

        Ok(())
    }
}
