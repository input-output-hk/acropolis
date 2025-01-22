//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, multi-protocol client interface to the Cardano node

use caryatid_sdk::{Context, Module, module, MessageBounds};
use acropolis_messages::MiniprotocolIncomingMessage;
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{info, error};

use pallas::network::{
    facades::PeerClient,
};

const DEFAULT_TOPIC: &str = "cardano.network.incoming.";
const DEFAULT_NODE_ADDRESS: &str = "preview-node.world.dev.cardano.org:30002";
const DEFAULT_MAGIC_NUMBER: u64 = 2;

/// Network mini-protocols module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(M),
    name = "miniprotocols",
    description = "Mini-protocol interface to the Cardano node"
)]
pub struct Miniprotocols<M: From<MiniprotocolIncomingMessage> + MessageBounds>;

impl<M: From<MiniprotocolIncomingMessage> + MessageBounds> Miniprotocols<M>
{
    fn init(&self, context: Arc<Context<M>>, config: Arc<Config>) -> Result<()> {
        let _message_bus = context.message_bus.clone();
        let _topic = config.get_string("topic").unwrap_or(DEFAULT_TOPIC.to_string());

        let node_address = config.get_string("node_address")
            .unwrap_or(DEFAULT_NODE_ADDRESS.to_string());
        let magic_number: u64 = config.get::<u64>("magic_number")
            .unwrap_or(DEFAULT_MAGIC_NUMBER);

        tokio::spawn(async move {
            info!("Connecting to {node_address} ({magic_number})");
            let peer = PeerClient::connect(node_address, magic_number).await;

            match peer {
                Ok(mut peer) => {
                    info!("Connected");

                    // Start a chain sync at the tip
                    let client = peer.chainsync();
                    client.intersect_tip().await.unwrap();

                    // Loop fetching messages
                    loop {
                        let next = if client.has_agency() {
                            client.request_next().await.unwrap()
                        } else {
                            client.recv_while_must_reply().await.unwrap()
                        };

                        info!("Incoming message: {next:?}");
                    }
                },
                Err(e) => error!("Failed to connect to peer: {e}")
            }
        });

        Ok(())
    }
}
