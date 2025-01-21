//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, multi-protocol client interface to the Cardano node

use caryatid_sdk::{Context, Module, module, MessageBounds};
use acropolis_messages::MiniprotocolIncomingMessage;
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, error};

const DEFAULT_TOPIC: &str = "cardano.network.incoming.";

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
        let message_bus = context.message_bus.clone();
        let topic = config.get_string("topic").unwrap_or(DEFAULT_TOPIC.to_string());

        Ok(())
    }
}
