//! Acropolis Block VRF Validator module for Caryatid
//! Validate the VRF calculation in the block header

use acropolis_common::{
    caryatid::SubscriptionExt,
    configuration::StartupMethod,
    messages::{
        AccountsBootstrapMessage, CardanoMessage, Message, SnapshotMessage, SnapshotStateMessage,
    },
    state_history::{StateHistory, StateHistoryStore},
    validation::ValidationOutcomes,
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};
mod state;
use state::State;
mod ouroboros;

mod snapshot;

const DEFAULT_VALIDATION_VRF_PUBLISHER_TOPIC: (&str, &str) =
    ("validation-vrf-publisher-topic", "cardano.validation.vrf");

const DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC: (&str, &str) = (
    "bootstrapped-subscribe-topic",
    "cardano.sequence.bootstrapped",
);
const DEFAULT_BLOCK_SUBSCRIBE_TOPIC: (&str, &str) =
    ("block-subscribe-topic", "cardano.block.proposed");
const DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC: (&str, &str) = (
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
);
const DEFAULT_EPOCH_NONCE_SUBSCRIBE_TOPIC: (&str, &str) =
    ("epoch-nonce-subscribe-topic", "cardano.epoch.nonce");
const DEFAULT_SPO_STATE_SUBSCRIBE_TOPIC: (&str, &str) =
    ("spo-state-subscribe-topic", "cardano.spo.state");
const DEFAULT_SPDD_SUBSCRIBE_TOPIC: (&str, &str) =
    ("spdd-subscribe-topic", "cardano.spo.distribution");
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

/// Block VRF Validator module
#[module(
    message_type(Message),
    name = "block-vrf-validator",
    description = "Validate the VRF calculation in the block header"
)]

pub struct BlockVrfValidator;

impl BlockVrfValidator {
    /// Handle bootstrap message from snapshot
    fn handle_bootstrap(state: &mut State, vrf_data: AccountsBootstrapMessage) -> Result<()> {
        let epoch = vrf_data.epoch;
        let pools_len = vrf_data.pools.len();

        // Initialize VRF validator state from snapshot data
        state.bootstrap(vrf_data)?;

        info!(
            "VRF state bootstrapped successfully for epoch {} with {} pools",
            epoch, pools_len
        );

        Ok(())
    }

    /// Wait for and process snapshot bootstrap messages
    async fn wait_for_bootstrap(
        history: Arc<Mutex<StateHistory<State>>>,
        mut snapshot_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        info!("Waiting for snapshot bootstrap messages...");
        loop {
            let (_, message) = snapshot_subscription.read().await?;
            let message = Arc::try_unwrap(message).unwrap_or_else(|arc| (*arc).clone());

            match message {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("Received snapshot startup signal, awaiting bootstrap data...");
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(
                    SnapshotStateMessage::AccountsState(accounts_data),
                )) => {
                    info!("Received AccountsState bootstrap message");

                    let block_number = accounts_data.block_number;
                    let mut state = State::default();

                    Self::handle_bootstrap(&mut state, accounts_data)?;
                    history.lock().await.bootstrap_init_with(state, block_number);
                    info!("VRF validator bootstrap complete");
                }
                Message::Snapshot(SnapshotMessage::Complete) => {
                    info!("Snapshot complete, exiting VRF validator bootstrap loop");
                    return Ok(());
                }
                _ => {
                    // Ignore other messages (e.g., EpochState, SPOState bootstrap messages)
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run(
        context: Arc<Context<Message>>,
        history: Arc<Mutex<StateHistory<State>>>,
        mut bootstrapped_subscription: Box<dyn Subscription<Message>>,
        mut block_subscription: Box<dyn Subscription<Message>>,
        mut protocol_parameters_subscription: Box<dyn Subscription<Message>>,
        mut epoch_nonce_subscription: Box<dyn Subscription<Message>>,
        mut spo_state_subscription: Box<dyn Subscription<Message>>,
        mut spdd_subscription: Box<dyn Subscription<Message>>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        publish_vrf_validation_topic: String,
    ) -> Result<()> {
        let (_, bootstrapped_message) = bootstrapped_subscription.read().await?;
        let genesis = match bootstrapped_message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                complete.values.clone()
            }
            _ => panic!("Unexpected message in genesis completion topic: {bootstrapped_message:?}"),
        };

        // Consume initial protocol parameters or bootstap message
        if let Some(snapshot_subscription) = snapshot_subscription {
            Self::wait_for_bootstrap(history.clone(), snapshot_subscription).await?;
        } else {
            let _ = protocol_parameters_subscription.read().await?;
        }

        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_or_init_with(State::new);
            let mut current_block: Option<BlockInfo> = None;

            let (_, message) = block_subscription.read_ignoring_rollbacks().await?;
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
                            Message::Cardano((
                                block_info,
                                CardanoMessage::EpochNonce(active_nonce),
                            )) => {
                                Self::check_sync(&current_block, block_info);
                                state.handle_epoch_nonce(active_nonce);
                            }
                            _ => error!("Unexpected message type: {epoch_nonce_msg:?}"),
                        });

                        let (_, spo_state_msg) =
                            spo_state_subscription.read_ignoring_rollbacks().await?;
                        let (_, spdd_msg) = spdd_subscription.read_ignoring_rollbacks().await?;
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
                                state.handle_new_snapshot(spo_state_msg, spdd_msg);
                            }
                            _ => {
                                error!("Unexpected message type: {spo_state_msg:?} or {spdd_msg:?}")
                            }
                        });
                    }

                    let span =
                        info_span!("block_vrf_validator.validate", block = block_info.number);
                    async {
                        let mut validation_outcomes = ValidationOutcomes::new();
                        if let Err(e) = state.validate(block_info, &block_msg.header, &genesis) {
                            validation_outcomes.push(*e);
                        }

                        validation_outcomes
                            .publish(&context, &publish_vrf_validation_topic, block_info)
                            .await
                            .unwrap_or_else(|e| error!("Failed to publish VRF validation: {e}"));
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

        let block_subscribe_topic = config
            .get_string(DEFAULT_BLOCK_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCK_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating block subscription on '{block_subscribe_topic}'");

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

        let snapshot_subscribe_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());

        // Subscribers
        let snapshot_subscription = if StartupMethod::from_config(config.as_ref()).is_snapshot() {
            info!("Creating subscriber for snapshot on '{snapshot_subscribe_topic}'");
            Some(context.subscribe(&snapshot_subscribe_topic).await?)
        } else {
            info!("Skipping snapshot subscription (startup method is not snapshot)");
            None
        };
        let bootstrapped_subscription = context.subscribe(&bootstrapped_subscribe_topic).await?;
        let protocol_parameters_subscription =
            context.subscribe(&protocol_parameters_subscribe_topic).await?;
        let block_subscription = context.subscribe(&block_subscribe_topic).await?;
        let epoch_nonce_subscription = context.subscribe(&epoch_nonce_subscribe_topic).await?;
        let spo_state_subscription = context.subscribe(&spo_state_subscribe_topic).await?;
        let spdd_subscription = context.subscribe(&spdd_subscribe_topic).await?;

        // state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "block_vrf_validator",
            StateHistoryStore::default_block_store(),
        )));

        // Start run task
        let context_run = context.clone();
        context.run(async move {
            Self::run(
                context_run,
                history,
                bootstrapped_subscription,
                block_subscription,
                protocol_parameters_subscription,
                epoch_nonce_subscription,
                spo_state_subscription,
                spdd_subscription,
                snapshot_subscription,
                validation_vrf_publisher_topic,
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
