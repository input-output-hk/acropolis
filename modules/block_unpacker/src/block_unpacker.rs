//! Acropolis Block unpacker module for Caryatid
//! Unpacks block bodies into transactions

use caryatid_sdk::{Context, Module, module, MessageBounds, MessageBusExt};
use acropolis_messages::{BlockBodyMessage, TxMessage, Message};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};

use pallas::{
};

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.block.body";
const DEFAULT_PUBLISH_TOPIC: &str = "cardano.tx";

/// Block unpacker module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "block-unpacker",
    description = "Block to transaction unpacker"
)]
pub struct BlockUnpacker;

impl BlockUnpacker
{
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Subscribe for block body messages
        // Get configuration
        let topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{}'", topic);

        context.message_bus.subscribe(&topic,
                                      move |message: Arc<Message>| {
           match message.as_ref() {
               Message::BlockBody(body_msg) => {
                   info!("Received block {}", body_msg.slot);

                   // TODO parse body and publish transactions
               }

               _ => error!("Unexpected message type: {message:?}")
           }
        })?;

        Ok(())
    }
}
