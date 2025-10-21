//! Acropolis consensus module for Caryatid
//! Maintains a favoured chain based on offered options from multiple sources

use acropolis_common::messages::{CardanoMessage, Message};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::sync::Arc;
use tracing::{error, info, info_span, Instrument};

const DEFAULT_SUBSCRIBE_BLOCKS_TOPIC: &str = "cardano.block.available";
const DEFAULT_PUBLISH_BLOCKS_TOPIC: &str = "cardano.block.proposed";

/// Consensus module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "consensus",
    description = "Consensus algorithm"
)]
pub struct Consensus;

impl Consensus {
    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Subscribe for block messages
        // Get configuration
        let subscribe_blocks_topic = config.get_string("subscribe-blocks-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_BLOCKS_TOPIC.to_string());
        info!("Creating blocks subscriber on '{subscribe_blocks_topic}'");

        let publish_blocks_topic = config.get_string("publish-blocks topic")
            .unwrap_or(DEFAULT_PUBLISH_BLOCKS_TOPIC.to_string());
        info!("Publishing blocks on '{publish_blocks_topic}'");

        let mut subscription = context.subscribe(&subscribe_blocks_topic).await?;

        // TODO Subscribe for validation errors
        // TODO Reject and rollback blocks if validation fails

        context.clone().run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::BlockAvailable(_block_msg))) => {
                        let span = info_span!("consensus", block = block_info.number);

                        async {
                            // TODO Actually decide on favoured chain!
                            context
                                .message_bus
                                .publish(&publish_blocks_topic, message.clone())
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                        }
                        .instrument(span)
                            .await;
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        Ok(())
    }
}
