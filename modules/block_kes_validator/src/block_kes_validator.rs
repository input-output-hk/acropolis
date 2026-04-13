//! Acropolis Block KES Validator module for Caryatid
//! Validate KES signatures in the block header

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    configuration::{get_string_flag, StartupMode},
    declare_cardano_reader,
    messages::{
        BlockKesValidatorBootstrapMessage, CardanoMessage, Message, ProtocolParamsMessage,
        RawBlockMessage, SPOStateMessage, SnapshotMessage, SnapshotStateMessage,
        StateTransitionMessage,
    },
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

use crate::ouroboros::kes_validation::op_cert_counter_no_validation;
mod ouroboros;

const DEFAULT_VALIDATION_KES_PUBLISHER_TOPIC: (&str, &str) =
    ("validation-kes-publisher-topic", "cardano.validation.kes");

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
    SPOReader,
    "spo-state-subscribe-topic",
    "cardano.spo.state",
    SPOState,
    SPOStateMessage
);

/// Block KES Validator module
#[module(
    message_type(Message),
    name = "block-kes-validator",
    description = "Validate the KES signatures in the block header"
)]

pub struct BlockKesValidator;

impl BlockKesValidator {
    /// Handle bootstrap message from snapshot
    fn handle_bootstrap(state: &mut State, kes_data: BlockKesValidatorBootstrapMessage) {
        let epoch = kes_data.epoch;
        let counters_len = kes_data.ocert_counters.len();

        // Initialize KES validator state from snapshot data
        state.bootstrap(kes_data.ocert_counters);

        info!(
            "KES state bootstrapped successfully for epoch {} with {} opcert counters",
            epoch, counters_len
        );
    }

    /// Wait for and process snapshot bootstrap messages
    async fn wait_for_bootstrap(
        history: Arc<Mutex<StateHistory<State>>>,
        mut snapshot_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        info!("Waiting for KES validator snapshot bootstrap messages...");
        loop {
            let (_, message) = snapshot_subscription.read().await?;
            let message = Arc::try_unwrap(message).unwrap_or_else(|arc| (*arc).clone());

            match message {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("Received snapshot startup signal, awaiting KES bootstrap data...");
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(
                    SnapshotStateMessage::BlockKesValidatorState(kes_data),
                )) => {
                    info!("Received BlockKesValidatorState bootstrap message");

                    let block_number = kes_data.block_number;
                    let mut state = State::new();

                    Self::handle_bootstrap(&mut state, kes_data);
                    history.lock().await.bootstrap_init_with(state, block_number);
                    info!("KES validator bootstrap complete");
                }
                Message::Snapshot(SnapshotMessage::Complete) => {
                    info!("Snapshot complete, exiting KES validator bootstrap loop");
                    return Ok(());
                }
                _ => {}
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
        mut spo_state_reader: SPOReader,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        kes_validation_topic: String,
    ) -> Result<()> {
        let (_, bootstrapped_message) = bootstrapped_subscription.read().await?;
        let genesis = match bootstrapped_message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                complete.values.clone()
            }
            _ => panic!("Unexpected message in genesis completion topic: {bootstrapped_message:?}"),
        };

        // Consume initial protocol parameters or bootstrap message
        if let Some(subscription) = snapshot_subscription {
            Self::wait_for_bootstrap(history.clone(), subscription).await?;
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
            let mut ctx =
                ValidationContext::new(&context, &kes_validation_topic, "block_kes_validator");

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

            if primary.should_read_epoch_transition_messages() {
                match ctx
                    .consume_sync("params_reader", params_reader.read_with_rollbacks().await)?
                {
                    RollbackWrapper::Normal((_, params)) => {
                        state.handle_protocol_parameters(&params);
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                match ctx.consume_sync(
                    "spo_state_reader",
                    spo_state_reader.read_with_rollbacks().await,
                )? {
                    RollbackWrapper::Normal((_, spo_state)) => {
                        state.handle_spo_state(&spo_state);
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            if let Some(block_msg) = primary.message() {
                let block_info = primary.block_info().clone();
                if primary.do_validation() {
                    let span =
                        info_span!("block_kes_validator.validate", block = block_info.number);
                    async {
                        let result_opt = ctx.handle(
                            "validate",
                            state
                                .validate(&block_info, &block_msg.header, &genesis)
                                .map_err(anyhow::Error::from),
                        );

                        if let Some((pool_id, updated_sequence_number)) = result_opt {
                            // Update the operational certificate counter
                            // When block is validated successfully
                            // Reference
                            // https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos.hs#L508
                            state.update_ocert_counter(pool_id, updated_sequence_number);
                        }
                    }
                    .instrument(span)
                    .await;

                    // Publish validation outcomes
                    ctx.publish().await;
                } else if let Some((pool_id, updated_sequence_number)) =
                    op_cert_counter_no_validation(&block_msg.header, &block_info)
                {
                    state.update_ocert_counter(pool_id, updated_sequence_number);
                }

                // Commit the new state
                history.lock().await.commit(block_info.number, state);
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Publish topics
        let kes_validation_topic = get_string_flag(&config, DEFAULT_VALIDATION_KES_PUBLISHER_TOPIC);
        info!("Creating validation KES publisher on '{kes_validation_topic}'");

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
        let param_reader = ParamsReader::new(&context, &config).await?;
        let spo_state_reader = SPOReader::new(&context, &config).await?;

        // state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "block_kes_validator",
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
                param_reader,
                spo_state_reader,
                snapshot_subscription,
                kes_validation_topic,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
