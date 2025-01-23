//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, multi-protocol client interface to the Cardano node

use caryatid_sdk::{Context, Module, module, MessageBounds};
use acropolis_messages::{NewTipHeaderMessage};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};

use pallas::{
    network::{
        facades::PeerClient,
        miniprotocols::{
            chainsync::{NextResponse, Tip},
        },
    },
    ledger::{
        traverse::MultiEraHeader,
    }
};

const DEFAULT_TOPIC: &str = "cardano.network.new.tip.header";
const DEFAULT_NODE_ADDRESS: &str = "preview-node.world.dev.cardano.org:30002";
const DEFAULT_MAGIC_NUMBER: u64 = 2;

/// Network mini-protocols module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(M),
    name = "miniprotocols",
    description = "Mini-protocol interface to the Cardano node"
)]
pub struct Miniprotocols<M: From<NewTipHeaderMessage> + MessageBounds>;

impl<M: From<NewTipHeaderMessage> + MessageBounds> Miniprotocols<M>
{
    /// ChainSync client loop
    async fn run_chain_sync(context: Arc<Context<M>>, config: Arc<Config>,
                            mut peer: PeerClient) -> Result<()> {
        let topic = config.get_string("topic").unwrap_or(DEFAULT_TOPIC.to_string());

        // Start a chain sync at the tip
        let client = peer.chainsync();
        client.intersect_tip().await?;

        // Loop fetching messages
        loop {
            let next = client.request_or_await_next().await?;

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
                                    let message = NewTipHeaderMessage {
                                        slot: header.slot(),
                                        number: header.number(),
                                        raw: h.cbor
                                    };

                                    debug!("Miniprotocols sending {:?}", message);

                                    let message_enum: M = message.into();
                                    context.message_bus.publish(&topic, Arc::new(message_enum))
                                        .await
                                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                                }
                                Err(e) => error!("Bad header: {e}"),
                            }
                        },
                        Some(_) => info!("Skipping a Byron block"),
                    }
                },

                _ => debug!("Ignoring message: {next:?}")
            }
        }
    }

    /// Main init function
    pub fn init(&self, context: Arc<Context<M>>, config: Arc<Config>) -> Result<()> {
        let node_address = config.get_string("node_address")
            .unwrap_or(DEFAULT_NODE_ADDRESS.to_string());
        let magic_number: u64 = config.get::<u64>("magic_number")
            .unwrap_or(DEFAULT_MAGIC_NUMBER);

        tokio::spawn(async move {
            info!("Connecting to {node_address} ({magic_number})");
            let peer = PeerClient::connect(node_address, magic_number).await;

            match peer {
                Ok(peer) => {
                    info!("Connected");
                    Self::run_chain_sync(context, config, peer)
                        .await
                        .unwrap_or_else(|e| error!("Chain sync failed: {e}"));
                },
                Err(e) => error!("Failed to connect to peer: {e}")
            }
        });

        Ok(())
    }
}
