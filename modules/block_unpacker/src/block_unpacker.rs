//! Acropolis Block unpacker module for Caryatid
//! Unpacks block bodies into transactions

use acropolis_common::messages::{CardanoMessage, Message, RawTxsMessage};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use pallas::ledger::traverse::MultiEraBlock;
use std::sync::Arc;
use tracing::{debug, error, info, info_span, Instrument};

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.block.available";
const DEFAULT_PUBLISH_TOPIC: &str = "cardano.txs";

/// Block unpacker module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "block-unpacker",
    description = "Block to transaction unpacker"
)]
pub struct BlockUnpacker;

impl BlockUnpacker {
    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Subscribe for block body messages
        // Get configuration
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let publish_topic =
            config.get_string("publish-topic").unwrap_or(DEFAULT_PUBLISH_TOPIC.to_string());
        info!("Publishing on '{publish_topic}'");

        let mut subscription = context.subscribe(&subscribe_topic).await?;

        context.clone().run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::BlockAvailable(block_msg))) => {
                        // Parse the body
                        match MultiEraBlock::decode(&block_msg.body) {
                            Ok(block) => {
                                let span = info_span!("block_unpacker", block = block_info.number);

                                async {
                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!(
                                            "Decoded block number {} slot {} with {} txs",
                                            block.number(),
                                            block.slot(),
                                            block.txs().len()
                                        );
                                    }

                                    // Encode the Tx into hex, and take ownership
                                    let txs: Vec<_> =
                                        block.txs().into_iter().map(|tx| tx.encode()).collect();

                                    let tx_message = RawTxsMessage { txs };
                                    let message_enum = Message::Cardano((
                                        block_info.clone(),
                                        CardanoMessage::ReceivedTxs(tx_message),
                                    ));
                                    context
                                        .message_bus
                                        .publish(&publish_topic, Arc::new(message_enum))
                                        .await
                                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                                }
                                .instrument(span)
                                .await;
                            }

                            Err(e) => error!("Can't decode block {}: {e}", block_info.number),
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        Ok(())
    }
}
