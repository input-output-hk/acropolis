//! Acropolis consensus module for Caryatid
//! Maintains a favoured chain based on offered options from multiple sources

use acropolis_common::{messages::{CardanoMessage, Message, StateTransitionMessage}, validation::ValidationStatus, BlockIntent};
use anyhow::Result;
use caryatid_sdk::{module, Context};
use config::Config;
use futures::future::try_join_all;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{error, info, info_span, Instrument};

const DEFAULT_SUBSCRIBE_BLOCKS_TOPIC: &str = "cardano.block.available";
const DEFAULT_PUBLISH_BLOCKS_TOPIC: &str = "cardano.block.proposed";
const DEFAULT_VALIDATION_TIMEOUT: i64 = 60; // seconds

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
        let subscribe_blocks_topic = config
            .get_string("subscribe-blocks-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_BLOCKS_TOPIC.to_string());
        info!("Creating blocks subscriber on '{subscribe_blocks_topic}'");

        let publish_blocks_topic = config
            .get_string("publish-blocks-topic")
            .unwrap_or(DEFAULT_PUBLISH_BLOCKS_TOPIC.to_string());
        info!("Publishing blocks on '{publish_blocks_topic}'");

        let validator_topics: Vec<String> =
            config.get::<Vec<String>>("validators").unwrap_or_default();
        for topic in &validator_topics {
            info!("Validator: {topic}");
        }

        let validation_timeout = Duration::from_secs(
            config.get_int("validation-timeout").unwrap_or(DEFAULT_VALIDATION_TIMEOUT) as u64,
        );
        info!("Validation timeout {validation_timeout:?}");

        // Subscribe for incoming blocks
        let mut subscription = context.subscribe(&subscribe_blocks_topic).await?;

        // Subscribe all the validators
        let mut validator_subscriptions: Vec<_> =
            try_join_all(validator_topics.iter().map(|topic| context.subscribe(topic))).await?;

        // True if we expect validation to be performed by the nodes
        let do_validation = validator_subscriptions.len() > 0;

        context.clone().run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    error!("Block message read failed");
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((raw_blk_info, ba @ CardanoMessage::BlockAvailable(_))) => {
                        let block_info = if do_validation {
                            raw_blk_info.with_intent(BlockIntent::ValidateAndApply)
                        }
                        else {
                            raw_blk_info.clone()
                        };
                        let block = (block_info.clone(), ba.clone());
                        let block = Arc::new(Message::Cardano(block));

                        let span = info_span!("consensus", block = block_info.number);

                        async {
                            // TODO Actually decide on favoured chain!

                            // Send to all validators and state modules
                            context
                                .message_bus
                                .publish(&publish_blocks_topic, block)
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                            // Read validation responses from all validators in parallel
                            // and check they are all positive, with a safety timeout
                            let all_say_go = match timeout(
                                validation_timeout,
                                try_join_all(validator_subscriptions.iter_mut().map(|s| s.read())),
                            )
                            .await
                            {
                                Ok(Ok(results)) => {
                                    results.iter().fold(true, |all_ok, (_topic, msg)| {
                                        match msg.as_ref() {
                                            Message::Cardano((
                                                block_info,
                                                CardanoMessage::BlockValidation(status),
                                            )) => match status {
                                                ValidationStatus::Go => all_ok,
                                                ValidationStatus::NoGo(err) => {
                                                    error!(
                                                        block = block_info.number,
                                                        ?err,
                                                        "Validation failure"
                                                    );
                                                    false
                                                }
                                            },

                                            _ => {
                                                error!(
                                                    "Unexpected validation message type: {msg:?}"
                                                );
                                                false
                                            }
                                        }
                                    })
                                }
                                Ok(Err(e)) => {
                                    error!("Failed to read validations: {e}");
                                    false
                                }
                                Err(_) => {
                                    error!("Timeout waiting for validation responses");
                                    false
                                }
                            };

                            if !all_say_go {
                                error!(block = block_info.number, "Validation rejected block");
                                // TODO Consequences:  rollback, blacklist source
                            }
                        }
                        .instrument(span)
                        .await;
                    }

                    Message::Cardano((
                        _,
                        CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                    )) => {
                        // Send rollback to all validators and state modules
                        context
                            .message_bus
                            .publish(&publish_blocks_topic, message.clone())
                            .await
                            .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        Ok(())
    }
}
