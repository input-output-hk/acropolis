//! Acropolis historical epochs state module for Caryatid
//! Manages optional state data needed for Blockfrost alignment

use crate::immutable_historical_epochs_state::ImmutableHistoricalEpochsState;
use crate::state::{HistoricalEpochsStateConfig, State};
use acropolis_common::caryatid::{PrimaryRead, RollbackWrapper};
use acropolis_common::configuration::StartupMode;
use acropolis_common::declare_cardano_reader;
use acropolis_common::messages::{
    EpochActivityMessage, ProtocolParamsMessage, RawBlockMessage, StateQuery,
    StateTransitionMessage,
};
use acropolis_common::queries::epochs::{
    EpochInfo, EpochsStateQuery, NextEpochs, PreviousEpochs, DEFAULT_HISTORICAL_EPOCHS_QUERY_TOPIC,
};
use acropolis_common::{
    messages::{CardanoMessage, Message, StateQueryResponse},
    queries::epochs::EpochsStateQueryResponse,
    queries::errors::QueryError,
    BlockStatus,
};
use anyhow::{bail, Result};
use caryatid_sdk::{message_bus::Subscription, module, Context};
use config::Config;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};
mod immutable_historical_epochs_state;
mod state;
mod volatile_historical_epochs_state;

// Configuration defaults
declare_cardano_reader!(
    BlockReader,
    "blocks-subscribe-topic",
    "cardano.block.proposed",
    BlockAvailable,
    RawBlockMessage
);
declare_cardano_reader!(
    EpochActivityReader,
    "epoch-activity-subscribe-topic",
    "cardano.epoch.activity",
    EpochActivity,
    EpochActivityMessage
);
declare_cardano_reader!(
    ParamsReader,
    "parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);

const DEFAULT_HISTORICAL_EPOCHS_STATE_DB_PATH: (&str, &str) = ("db-path", "./fjall-epochs");
const DEFAULT_CLEAR_ON_START: (&str, bool) = ("clear-on-start", true);

/// Historical Epochs State module
#[module(
    message_type(Message),
    name = "historical-epochs-state",
    description = "Historical epochs state for Blockfrost compatibility"
)]
pub struct HistoricalEpochsState;

impl HistoricalEpochsState {
    /// Async run loop
    async fn run(
        state_mutex: Arc<Mutex<State>>,
        mut blocks_reader: BlockReader,
        mut epoch_activity_reader: EpochActivityReader,
        mut params_reader: ParamsReader,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        if !is_snapshot_mode {
            loop {
                match params_reader.read_with_rollbacks().await? {
                    RollbackWrapper::Normal(_) => {
                        debug!("Consumed initial genesis params from params_subscription");
                        break;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }
        }

        // Background task to persist epoch sequentially
        const MAX_PENDING_PERSISTS: usize = 1;
        let (persist_tx, mut persist_rx) =
            mpsc::channel::<(u64, Arc<ImmutableHistoricalEpochsState>)>(MAX_PENDING_PERSISTS);
        tokio::spawn(async move {
            while let Some((epoch, store)) = persist_rx.recv().await {
                if let Err(e) = store.persist_epoch(epoch).await {
                    error!("failed to persist epoch {epoch}: {e}");
                }
            }
        });

        // Main loop of synchronised messages
        loop {
            // Use blocks_message as the synchroniser
            let primary = PrimaryRead::from_read(blocks_reader.read_with_rollbacks().await?);

            if primary.is_rollback() || primary.block_info().status == BlockStatus::RolledBack {
                let mut state = state_mutex.lock().await;
                state.volatile.rollback_before(primary.block_info().number);
            }

            let epoch = primary.epoch();

            // Protocol parameters publish on any new epoch, including epoch 0.
            if primary.should_read_epoch_messages() {
                match params_reader.read_with_rollbacks().await? {
                    RollbackWrapper::Normal((_, params)) => {
                        let mut state = state_mutex.lock().await;
                        if let Some(shelley) = &params.params.shelley {
                            state.volatile.update_k(shelley.security_param);
                        }
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            // Epoch activity publishes on real epoch transitions (>0) and rollbacks.
            if primary.should_read_epoch_transition_messages() {
                match epoch_activity_reader.read_with_rollbacks().await? {
                    RollbackWrapper::Normal((block_info, ea_msg)) if epoch.is_some() => {
                        let mut state = state_mutex.lock().await;
                        state.volatile.handle_new_epoch(&block_info, &ea_msg);
                    }
                    RollbackWrapper::Normal(_) | RollbackWrapper::Rollback(_) => {}
                }
            }

            // Prune volatile and persist if needed
            if primary.message().is_some() {
                let current_block = primary.block_info().clone();
                let should_prune = {
                    let state = state_mutex.lock().await;
                    state.ready_to_prune(&current_block)
                };

                if should_prune {
                    let immutable = {
                        let mut state = state_mutex.lock().await;
                        state.prune_volatile().await;
                        state.immutable.clone()
                    };

                    if let Err(e) = persist_tx.send((current_block.epoch, immutable)).await {
                        error!("persistence worker crashed: {e}");
                    }
                }
            }
        }
    }

    /// Async initialisation
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();

        // Query topic
        let historical_epochs_query_topic = config
            .get_string(DEFAULT_HISTORICAL_EPOCHS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_HISTORICAL_EPOCHS_QUERY_TOPIC.1.to_string());
        info!("Creating query handler on '{historical_epochs_query_topic}'");

        // Configuration
        let cfg = HistoricalEpochsStateConfig {
            db_path: config
                .get_string(DEFAULT_HISTORICAL_EPOCHS_STATE_DB_PATH.0)
                .unwrap_or(DEFAULT_HISTORICAL_EPOCHS_STATE_DB_PATH.1.to_string()),
            clear_on_start: config
                .get_bool(DEFAULT_CLEAR_ON_START.0)
                .unwrap_or(DEFAULT_CLEAR_ON_START.1),
        };

        // Initalize state
        let state = State::new(&cfg)?;
        let state_mutex = Arc::new(Mutex::new(state));
        let state_query = state_mutex.clone();

        context.handle(&historical_epochs_query_topic, move |message| {
            let state = state_query.clone();
            async move {
                let Message::StateQuery(StateQuery::Epochs(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Epochs(
                        EpochsStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for epochs-state",
                        )),
                    )));
                };

                let response = match query {
                    EpochsStateQuery::GetEpochInfo { epoch_number } => {
                        match state.lock().await.get_historical_epoch(*epoch_number) {
                            Ok(Some(epoch)) => {
                                EpochsStateQueryResponse::EpochInfo(EpochInfo { epoch })
                            }
                            Ok(None) => EpochsStateQueryResponse::Error(QueryError::not_found(
                                format!("Epoch {}", epoch_number),
                            )),
                            Err(e) => {
                                warn!("failed to get epoch info: {e}");
                                EpochsStateQueryResponse::Error(QueryError::internal_error(
                                    "historical epoch info",
                                ))
                            }
                        }
                    }

                    EpochsStateQuery::GetNextEpochs { epoch_number } => {
                        match state.lock().await.get_next_epochs(*epoch_number) {
                            Ok(epochs) => {
                                EpochsStateQueryResponse::NextEpochs(NextEpochs { epochs })
                            }
                            Err(e) => {
                                warn!("failed to get next epochs: {e}");
                                EpochsStateQueryResponse::Error(QueryError::internal_error(
                                    "historical next epochs",
                                ))
                            }
                        }
                    }

                    EpochsStateQuery::GetPreviousEpochs { epoch_number } => {
                        match state.lock().await.get_previous_epochs(*epoch_number) {
                            Ok(epochs) => {
                                EpochsStateQueryResponse::PreviousEpochs(PreviousEpochs { epochs })
                            }
                            Err(e) => {
                                warn!("failed to get previous epochs: {e}");
                                EpochsStateQueryResponse::Error(QueryError::internal_error(
                                    "historical previous epochs",
                                ))
                            }
                        }
                    }

                    _ => EpochsStateQueryResponse::Error(QueryError::not_implemented(format!(
                        "Unimplemented query variant: {query:?}"
                    ))),
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Epochs(
                    response,
                )))
            }
        });

        // Subscribe
        let blocks_reader = BlockReader::new(&context, &config).await?;
        let epoch_activity_reader = EpochActivityReader::new(&context, &config).await?;
        let params_reader = ParamsReader::new(&context, &config).await?;

        // Start run task
        context.run(async move {
            Self::run(
                state_mutex,
                blocks_reader,
                epoch_activity_reader,
                params_reader,
                is_snapshot_mode,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
