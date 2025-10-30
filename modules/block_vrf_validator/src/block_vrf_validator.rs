//! Acropolis Block VRF Validator module for Caryatid
//! Validate the VRF calculation in the block header

use acropolis_common::{
    messages::{CardanoMessage, Message},
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus, Era,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use pallas::ledger::traverse::MultiEraHeader;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};
mod state;
use state::State;

use crate::vrf_validation_publisher::VrfValidationPublisher;
mod snapshot;
mod vrf_validation_publisher;

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
const DEFAULT_EPOCH_NONCE_SUBSCRIBE_TOPIC: (&str, &str) =
    ("epoch-nonce-subscribe-topic", "cardano.epoch.nonce");
const DEFAULT_SPO_STATE_SUBSCRIBE_TOPIC: (&str, &str) =
    ("spo-state-subscribe-topic", "cardano.spo.state");
const DEFAULT_SPDD_SUBSCRIBE_TOPIC: (&str, &str) =
    ("spdd-subscribe-topic", "cardano.spo.distribution");

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
        mut vrf_validation_publisher: VrfValidationPublisher,
        mut bootstrapped_subscription: Box<dyn Subscription<Message>>,
        mut blocks_subscription: Box<dyn Subscription<Message>>,
        mut protocol_parameters_subscription: Box<dyn Subscription<Message>>,
        mut epoch_nonce_subscription: Box<dyn Subscription<Message>>,
        mut spo_state_subscription: Box<dyn Subscription<Message>>,
        mut spdd_subscription: Box<dyn Subscription<Message>>,
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
                        let epoch_nonce_message_f = epoch_nonce_subscription.read();
                        let spo_state_message_f = spo_state_subscription.read();
                        let spdd_msg_f = spdd_subscription.read();

                        let (_, protocol_parameters_msg) = protocol_parameters_message_f.await?;
                        let span = info_span!(
                            "block_vrf_validator.handle_protocol_parameters",
                            epoch = block_info.epoch
                        );
                        span.in_scope(|| match protocol_parameters_msg.as_ref() {
                            Message::Cardano((block_info, CardanoMessage::ProtocolParams(msg))) => {
                                Self::check_sync(&current_block, block_info);
                                state.handle_protocol_parameters(msg);
                            }
                            _ => error!("Unexpected message type: {protocol_parameters_msg:?}"),
                        });

                        let (_, epoch_nonce_msg) = epoch_nonce_message_f.await?;
                        let span = info_span!(
                            "block_vrf_validator.handle_epoch_nonce",
                            epoch = block_info.epoch
                        );
                        span.in_scope(|| match epoch_nonce_msg.as_ref() {
                            Message::Cardano((block_info, CardanoMessage::EpochNonce(msg))) => {
                                Self::check_sync(&current_block, block_info);
                                state.handle_epoch_nonce(msg);
                            }
                            _ => error!("Unexpected message type: {epoch_nonce_msg:?}"),
                        });

                        let (_, spo_state_msg) = spo_state_message_f.await?;
                        let (_, spdd_msg) = spdd_msg_f.await?;
                        let span = info_span!(
                            "block_vrf_validator.handle_new_snapshot",
                            epoch = block_info.epoch
                        );
                        span.in_scope(|| match (spo_state_msg.as_ref(), spdd_msg.as_ref()) {
                            (
                                Message::Cardano((
                                    block_info_1,
                                    CardanoMessage::SPOState(spo_state_msg),
                                )),
                                Message::Cardano((
                                    block_info_2,
                                    CardanoMessage::SPOStakeDistribution(spdd_msg),
                                )),
                            ) => {
                                Self::check_sync(&current_block, block_info_1);
                                Self::check_sync(&current_block, block_info_2);
                                state.handle_new_snapshot(&spo_state_msg, &spdd_msg);
                            }
                            _ => {
                                error!("Unexpected message type: {spo_state_msg:?} or {spdd_msg:?}")
                            }
                        });
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
                    async {
                        if let Some(header) = header.as_ref() {
                            let result = state.validate_block_vrf(block_info, header, &genesis);
                            if let Err(e) = vrf_validation_publisher
                                .publish_vrf_validation(block_info, result)
                                .await
                            {
                                error!("Failed to publish VRF validation: {e}")
                            }
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

        let epoch_nonce_subscribe_topic = config
            .get_string(DEFAULT_EPOCH_NONCE_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCH_NONCE_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating epoch nonce subscription on '{epoch_nonce_subscribe_topic}'");

        let spo_state_subscribe_topic = config
            .get_string(DEFAULT_SPO_STATE_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SPO_STATE_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating spo state subscription on '{spo_state_subscribe_topic}'");

        let spdd_subscribe_topic = config
            .get_string(DEFAULT_SPDD_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SPDD_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating spdd subscription on '{spdd_subscribe_topic}'");

        // publishers
        let vrf_validation_publisher =
            VrfValidationPublisher::new(context.clone(), validation_vrf_publisher_topic);

        // Subscribers
        let bootstrapped_subscription = context.subscribe(&bootstrapped_subscribe_topic).await?;
        let protocol_parameters_subscription =
            context.subscribe(&protocol_parameters_subscribe_topic).await?;
        let blocks_subscription = context.subscribe(&blocks_subscribe_topic).await?;
        let epoch_nonce_subscription = context.subscribe(&epoch_nonce_subscribe_topic).await?;
        let spo_state_subscription = context.subscribe(&spo_state_subscribe_topic).await?;
        let spdd_subscription = context.subscribe(&spdd_subscribe_topic).await?;

        // state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "block_vrf_validator",
            StateHistoryStore::default_block_store(),
        )));

        // Start run task
        context.run(async move {
            Self::run(
                history,
                vrf_validation_publisher,
                bootstrapped_subscription,
                blocks_subscription,
                protocol_parameters_subscription,
                epoch_nonce_subscription,
                spo_state_subscription,
                spdd_subscription,
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
