//! Acropolis Block KES Validator module for Caryatid
//! Validate KES signatures in the block header

use acropolis_common::{
    messages::{CardanoMessage, Message},
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};
mod state;
use state::State;

use crate::kes_validation_publisher::KesValidationPublisher;
mod kes_validation_publisher;
mod ouroboros;

const DEFAULT_VALIDATION_KES_PUBLISHER_TOPIC: (&str, &str) =
    ("validation-kes-publisher-topic", "cardano.validation.kes");

const DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC: (&str, &str) = (
    "bootstrapped-subscribe-topic",
    "cardano.sequence.bootstrapped",
);
const DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC: (&str, &str) = (
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
);
const DEFAULT_BLOCKS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("blocks-subscribe-topic", "cardano.block.proposed");

/// Block KES Validator module
#[module(
    message_type(Message),
    name = "block-kes-validator",
    description = "Validate the KES signatures in the block header"
)]

pub struct BlockKesValidator;

impl BlockKesValidator {
    #[allow(clippy::too_many_arguments)]
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut kes_validation_publisher: KesValidationPublisher,
        mut bootstrapped_subscription: Box<dyn Subscription<Message>>,
        mut blocks_subscription: Box<dyn Subscription<Message>>,
        mut protocol_parameters_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let (_, bootstrapped_message) = bootstrapped_subscription.read().await?;
        let genesis = match bootstrapped_message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                complete.values.clone()
            }
            _ => panic!("Unexpected message in genesis completion topic: {bootstrapped_message:?}"),
        };

        // Consume initial protocol parameters
        let _ = protocol_parameters_subscription.read().await?;

        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_or_init_with(State::new);
            let mut current_block: Option<BlockInfo> = None;

            let (_, message) = blocks_subscription.read().await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::BlockAvailable(block_msg))) => {
                    // handle rollback here
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    current_block = Some(block_info.clone());
                    let is_new_epoch = block_info.new_epoch && block_info.epoch > 0;

                    if is_new_epoch {
                        // read epoch boundary messages
                        let protocol_parameters_message_f = protocol_parameters_subscription.read();

                        let (_, protocol_parameters_msg) = protocol_parameters_message_f.await?;
                        let span = info_span!(
                            "block_kes_validator.handle_protocol_parameters",
                            epoch = block_info.epoch
                        );
                        span.in_scope(|| match protocol_parameters_msg.as_ref() {
                            Message::Cardano((block_info, CardanoMessage::ProtocolParams(msg))) => {
                                Self::check_sync(&current_block, block_info);
                                state.handle_protocol_parameters(msg);
                            }
                            _ => error!("Unexpected message type: {protocol_parameters_msg:?}"),
                        });
                    }

                    let span =
                        info_span!("block_kes_validator.validate", block = block_info.number);
                    async {
                        let result = state
                            .validate_block_kes(block_info, &block_msg.header, &genesis)
                            .map_err(|e| *e);
                        if let Err(e) = kes_validation_publisher
                            .publish_kes_validation(block_info, result)
                            .await
                        {
                            error!("Failed to publish KES validation: {e}")
                        }
                    }
                    .instrument(span)
                    .await;
                }
                _ => error!("Unexpected message type: {message:?}"),
            }

            // Commit the new state
            if let Some(block_info) = current_block {
                history.lock().await.commit(block_info.number, state);
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Publish topics
        let validation_kes_publisher_topic = config
            .get_string(DEFAULT_VALIDATION_KES_PUBLISHER_TOPIC.0)
            .unwrap_or(DEFAULT_VALIDATION_KES_PUBLISHER_TOPIC.1.to_string());
        info!("Creating validation KES publisher on '{validation_kes_publisher_topic}'");

        // Subscribe topics
        let bootstrapped_subscribe_topic = config
            .get_string(DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber for bootstrapped on '{bootstrapped_subscribe_topic}'");
        let protocol_parameters_subscribe_topic = config
            .get_string(DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber for protocol parameters on '{protocol_parameters_subscribe_topic}'");

        let blocks_subscribe_topic = config
            .get_string(DEFAULT_BLOCKS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCKS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating blocks subscription on '{blocks_subscribe_topic}'");

        // publishers
        let kes_validation_publisher =
            KesValidationPublisher::new(context.clone(), validation_kes_publisher_topic);

        // Subscribers
        let bootstrapped_subscription = context.subscribe(&bootstrapped_subscribe_topic).await?;
        let protocol_parameters_subscription =
            context.subscribe(&protocol_parameters_subscribe_topic).await?;
        let blocks_subscription = context.subscribe(&blocks_subscribe_topic).await?;

        // state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "block_kes_validator",
            StateHistoryStore::default_block_store(),
        )));

        // Start run task
        context.run(async move {
            Self::run(
                history,
                kes_validation_publisher,
                bootstrapped_subscription,
                blocks_subscription,
                protocol_parameters_subscription,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }

    /// Check for synchronisation
    fn check_sync(expected: &Option<BlockInfo>, actual: &BlockInfo) {
        if let Some(ref block) = expected {
            if block.number != actual.number {
                error!(
                    expected = block.number,
                    actual = actual.number,
                    "Messages out of sync"
                );
            }
        }
    }
}
