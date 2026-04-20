//! Acropolis Block VRF Validator module for Caryatid
//! Validate the VRF calculation in the block header

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    configuration::{get_string_flag, StartupMode},
    declare_cardano_reader,
    messages::{
        AccountsBootstrapMessage, CardanoMessage, Message, ProtocolParamsMessage, RawBlockMessage,
        SPOStakeDistributionMessage, SPOStateMessage, SnapshotMessage, SnapshotStateMessage,
        StateTransitionMessage,
    },
    protocol_params::Nonce,
    state_history::{StateHistory, StateHistoryStore},
};
use anyhow::{bail, Result};
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

const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

declare_cardano_reader!(
    BlockReader,
    "block-subscribe-topic",
    "cardano.block.proposed",
    BlockAvailable,
    RawBlockMessage
);

declare_cardano_reader!(
    ParamsReader,
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);

declare_cardano_reader!(
    NonceReader,
    "epoch-nonce-subscribe-topic",
    "cardano.epoch.nonce",
    EpochNonce,
    Option<Nonce>
);

declare_cardano_reader!(
    SPDDReader,
    "spdd-subscribe-topic",
    "cardano.spo.distribution",
    SPOStakeDistribution,
    SPOStakeDistributionMessage
);

declare_cardano_reader!(
    SPOReader,
    "spo-state-subscribe-topic",
    "cardano.spo.state",
    SPOState,
    SPOStateMessage
);

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
        mut block_reader: BlockReader,
        mut params_reader: ParamsReader,
        mut nonce_reader: NonceReader,
        mut spo_reader: SPOReader,
        mut spdd_reader: SPDDReader,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        publish_vrf_validation_topic: String,
    ) -> Result<()> {
        let (_, bootstrapped_message) = bootstrapped_subscription.read().await?;
        let genesis = match bootstrapped_message.as_ref() {
            Message::Cardano((block_info, CardanoMessage::GenesisComplete(complete))) => {
                let mut state = history.lock().await.get_or_init_with(State::new);
                state.handle_genesis(&complete.values, block_info);
                history.lock().await.commit(block_info.number, state);

                complete.values.clone()
            }
            _ => panic!("Unexpected message in genesis completion topic: {bootstrapped_message:?}"),
        };

        // Consume initial protocol parameters or bootstrap message.
        // Epoch 0 nonce comes from genesis; the first published epoch nonce
        // only arrives at the first epoch transition.
        if let Some(snapshot_subscription) = snapshot_subscription {
            Self::wait_for_bootstrap(history.clone(), snapshot_subscription).await?;
        } else {
            match params_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal((block_info, params)) => {
                    let mut state = history.lock().await.get_or_init_with(State::new);
                    state.handle_protocol_parameters(&params);
                    history.lock().await.commit(block_info.number, state);
                }
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial params");
                }
            }
        }

        loop {
            let mut ctx = ValidationContext::new(
                &context,
                &publish_vrf_validation_topic,
                "block_vrf_validator",
            );

            // Get a mutable state
            let mut state = history.lock().await.get_or_init_with(State::new);

            let primary = PrimaryRead::from_sync(
                &mut ctx,
                "block_reader",
                block_reader.read_with_rollbacks().await,
            )?;

            if primary.is_rollback() {
                state = history.lock().await.get_rolled_back_state(primary.block_info().number);
            }

            if primary.should_read_epoch_messages() {
                match ctx.consume("nonce_reader", nonce_reader.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal((_, active_nonce)) => {
                        state.handle_epoch_nonce(&active_nonce);
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            if primary.should_read_epoch_transition_messages() {
                // Read readers that publish new-epoch snapshots or rollback markers.
                match ctx.consume("params_reader", params_reader.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal((_, params)) => {
                        state.handle_protocol_parameters(&params);
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                let spo_state_msg =
                    match ctx.consume("spo_reader", spo_reader.read_with_rollbacks().await)? {
                        RollbackWrapper::Normal((_, spo_state)) => Some(spo_state),
                        RollbackWrapper::Rollback(_) => None,
                    };

                let spdd_msg =
                    match ctx.consume("spdd_reader", spdd_reader.read_with_rollbacks().await)? {
                        RollbackWrapper::Normal((_, spdd_msg)) => Some(spdd_msg),
                        RollbackWrapper::Rollback(_) => None,
                    };

                if let Some(spo_state_msg) = spo_state_msg {
                    if let Some(spdd_msg) = spdd_msg {
                        state.handle_new_snapshot(&spo_state_msg, &spdd_msg);
                    }
                }
            }

            if let Some(block_msg) = primary.message() {
                let block_info = primary.block_info().clone();
                if primary.do_validation() {
                    let span =
                        info_span!("block_vrf_validator.validate", block = block_info.number);
                    async {
                        ctx.handle(
                            "validate",
                            state
                                .validate(&block_info, &block_msg.header, &genesis)
                                .map_err(anyhow::Error::from),
                        );
                    }
                    .instrument(span)
                    .await;

                    // Publish validation outcomes
                    ctx.publish().await;
                }

                // Commit the new state
                history.lock().await.commit(block_info.number, state);
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Publish topics
        let validation_vrf_publisher_topic =
            get_string_flag(&config, DEFAULT_VALIDATION_VRF_PUBLISHER_TOPIC);
        info!("Creating validation VRF publisher on '{validation_vrf_publisher_topic}'");

        // Subscribe topics
        let bootstrapped_subscribe_topic =
            get_string_flag(&config, DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC);
        info!("Creating subscriber for bootstrapped on '{bootstrapped_subscribe_topic}'");

        let snapshot_subscribe_topic = get_string_flag(&config, DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC);

        // Subscribers
        let snapshot_subscription = if StartupMode::from_config(config.as_ref()).is_snapshot() {
            info!("Creating subscriber for snapshot on '{snapshot_subscribe_topic}'");
            Some(context.subscribe(&snapshot_subscribe_topic).await?)
        } else {
            info!("Skipping snapshot subscription (startup method is not snapshot)");
            None
        };
        let bootstrapped_subscription = context.subscribe(&bootstrapped_subscribe_topic).await?;

        let block_reader = BlockReader::new(&context, &config).await?;
        let params_reader = ParamsReader::new(&context, &config).await?;
        let nonce_reader = NonceReader::new(&context, &config).await?;
        let spo_reader = SPOReader::new(&context, &config).await?;
        let spdd_reader = SPDDReader::new(&context, &config).await?;

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
                block_reader,
                params_reader,
                nonce_reader,
                spo_reader,
                spdd_reader,
                snapshot_subscription,
                validation_vrf_publisher_topic,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
