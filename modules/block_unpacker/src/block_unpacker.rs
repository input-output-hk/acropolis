//! Acropolis Block unpacker module for Caryatid
//! Unpacks block bodies into transactions

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::messages::{RawTxsMessage, CardanoMessage, Message};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};
use pallas::ledger::traverse::MultiEraBlock;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.block.body";
const DEFAULT_PUBLISH_TOPIC: &str = "cardano.txs";

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
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let publish_topic = config.get_string("publish-topic")
            .unwrap_or(DEFAULT_PUBLISH_TOPIC.to_string());
        info!("Publishing on '{publish_topic}'");

        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let context = context.clone();
            let publish_topic = publish_topic.clone();

            async move {
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::BlockBody(body_msg))) => {
                        // Parse the body
                        match MultiEraBlock::decode(&body_msg.raw) {
                            Ok(block) => {
                                if tracing::enabled!(tracing::Level::DEBUG) {
                                    debug!("Decoded block number {} slot {} with {} txs",
                                           block.number(), block.slot(), block.txs().len());
                                }

                                let context = context.clone();
                                let publish_topic = publish_topic.clone();

                                // Encode the Tx into hex, and take ownership
                                let txs: Vec<_> = block.txs().into_iter()
                                    .map(|tx| tx.encode()).collect();

                                let tx_message = RawTxsMessage {
                                    txs
                                };
                                let message_enum = Message::Cardano((
                                    block_info.clone(),
                                    CardanoMessage::ReceivedTxs(tx_message)));
                                context.message_bus.publish(&publish_topic,
                                                            Arc::new(message_enum))
                                    .await
                                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                            },

                            Err(e) => error!("Can't decode block {}: {e}", block_info.number)
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        Ok(())
    }
}
