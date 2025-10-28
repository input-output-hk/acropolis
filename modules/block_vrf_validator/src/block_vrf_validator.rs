//! Acropolis Block VRF Validator module for Caryatid
//! Validate the VRF calculation in the block header
use acropolis_common::{
    messages::{CardanoMessage, Message},
    state_history::{StateHistory, StateHistoryStore},
    BlockStatus, Era,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use pallas::ledger::traverse::MultiEraHeader;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span};
mod state;
use state::State;

const DEFAULT_VALIDATION_VRF_PUBLISHER_TOPIC: (&str, &str) =
    ("validation-vrf-publisher-topic", "cardano.validation.vrf");

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
const DEFAULT_EPOCH_NONCES_SUBSCRIBE_TOPIC: (&str, &str) =
    ("epoch-nonces-subscribe-topic", "cardano.epoch.nonces");
const DEFAULT_SPO_STATE_SUBSCRIBE_TOPIC: (&str, &str) =
    ("spo-state-subscribe-topic", "cardano.spo.state");

/// Block VRF Validator module
#[module(
    message_type(Message),
    name = "block-vrf-validator",
    description = "Validate the VRF calculation in the block header"
)]

pub struct BlockVrfValidator;

impl BlockVrfValidator {
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut bootstrapped_subscription: Box<dyn Subscription<Message>>,
        mut blocks_subscription: Box<dyn Subscription<Message>>,
        mut protocol_parameters_subscription: Box<dyn Subscription<Message>>,
        mut epoch_nonces_subscription: Box<dyn Subscription<Message>>,
        mut spo_state_subscription: Box<dyn Subscription<Message>>,
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
            let mut state = history.lock().await.get_or_init_with(|| State::new());
            let (_, message) = blocks_subscription.read().await?;

            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::BlockAvailable(block_msg))) => {
                    // handle rollback here
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    let is_new_epoch = block_info.new_epoch && block_info.epoch > 0;

                    // read protocol parameters if new epoch
                    if is_new_epoch {
                        let protocol_parameters_message_f = protocol_parameters_subscription.read();
                        let epoch_nonces_message_f = epoch_nonces_subscription.read();
                        let (_, protocol_parameters_msg) = protocol_parameters_message_f.await?;
                        let (_, epoch_nonces_msg) = epoch_nonces_message_f.await?;

                        match protocol_parameters_msg.as_ref() {
                            Message::Cardano((_, CardanoMessage::ProtocolParams(msg))) => {
                                state.handle_protocol_parameters(msg);
                            }
                            _ => error!("Unexpected message type: {protocol_parameters_msg:?}"),
                        }

                        match epoch_nonces_msg.as_ref() {
                            Message::Cardano((_, CardanoMessage::EpochNonces(msg))) => {
                                state.handle_epoch_nonces(msg);
                            }
                            _ => error!("Unexpected message type: {epoch_nonces_msg:?}"),
                        };
                    }

                    // decode header
                    // Derive the variant from the era - just enough to make
                    // MultiEraHeader::decode() work.
                    let variant = match block_info.era {
                        Era::Byron => 0,
                        Era::Shelley => 1,
                        Era::Allegra => 2,
                        Era::Mary => 3,
                        Era::Alonzo => 4,
                        _ => 5,
                    };
                    let span = info_span!(
                        "block_vrf_validator.decode_header",
                        block = block_info.number
                    );
                    let mut header = None;
                    span.in_scope(|| {
                        header = match MultiEraHeader::decode(variant, None, &block_msg.header) {
                            Ok(header) => Some(header),
                            Err(e) => {
                                error!("Can't decode header {}: {e}", block_info.slot);
                                None
                            }
                        };
                    });

                    let span =
                        info_span!("block_vrf_validator.validate", block = block_info.number);
                    span.in_scope(|| {
                        if let Some(header) = header.as_ref() {
                            state.validate_block_vrf(block_info, header, &genesis);
                        }
                    });
                }
                _ => error!("Unexpected message type: {message:?}"),
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Publish topics
        let validation_vrf_publisher_topic = config
            .get_string(DEFAULT_VALIDATION_VRF_PUBLISHER_TOPIC.0)
            .unwrap_or(DEFAULT_VALIDATION_VRF_PUBLISHER_TOPIC.1.to_string());
        info!("Creating validation VRF publisher on '{validation_vrf_publisher_topic}'");

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

        let epoch_nonces_subscribe_topic = config
            .get_string(DEFAULT_EPOCH_NONCES_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCH_NONCES_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating epoch nonces subscription on '{epoch_nonces_subscribe_topic}'");

        let spo_state_subscribe_topic = config
            .get_string(DEFAULT_SPO_STATE_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SPO_STATE_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating spo state subscription on '{spo_state_subscribe_topic}'");

        // Subscribers
        let bootstrapped_subscription = context.subscribe(&bootstrapped_subscribe_topic).await?;
        let protocol_parameters_subscription =
            context.subscribe(&protocol_parameters_subscribe_topic).await?;
        let blocks_subscription = context.subscribe(&blocks_subscribe_topic).await?;
        let epoch_nonces_subscription = context.subscribe(&epoch_nonces_subscribe_topic).await?;
        let spo_state_subscription = context.subscribe(&spo_state_subscribe_topic).await?;

        // state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "block_vrf_validator",
            StateHistoryStore::default_block_store(),
        )));

        // Start run task
        context.run(async move {
            Self::run(
                history,
                bootstrapped_subscription,
                blocks_subscription,
                protocol_parameters_subscription,
                epoch_nonces_subscription,
                spo_state_subscription,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
