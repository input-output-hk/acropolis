//! Acropolis historical epochs state module for Caryatid
//! Manages optional state data needed for Blockfrost alignment

use crate::immutable_historical_epochs_state::ImmutableHistoricalEpochsState;
use crate::state::{HistoricalEpochsStateConfig, State};
use acropolis_common::messages::StateQuery;
use acropolis_common::queries::epochs::{
    EpochInfo, EpochsStateQuery, NextEpochs, PreviousEpochs, DEFAULT_HISTORICAL_EPOCHS_QUERY_TOPIC,
};
use acropolis_common::subscription::SubscriptionExt;
use acropolis_common::{
    messages::{CardanoMessage, Message, StateQueryResponse},
    queries::epochs::EpochsStateQueryResponse,
    queries::errors::QueryError,
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context};
use config::Config;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, info_span, warn, Instrument};
mod immutable_historical_epochs_state;
mod state;
mod volatile_historical_epochs_state;

// Configuration defaults
const DEFAULT_BLOCKS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("blocks-subscribe-topic", "cardano.block.proposed");
const DEFAULT_EPOCH_ACTIVITY_SUBSCRIBE_TOPIC: (&str, &str) =
    ("epoch-activity-subscribe-topic", "cardano.epoch.activity");
const DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("parameters-subscribe-topic", "cardano.protocol.parameters");

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
        mut blocks_subscription: Box<dyn Subscription<Message>>,
        mut epoch_activity_subscription: Box<dyn Subscription<Message>>,
        mut params_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let _ = params_subscription.read().await?;
        info!("Consumed initial genesis params from params_subscription");

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
            let mut current_block: Option<BlockInfo> = None;

            // Use certs_message as the synchroniser
            let (_, blocks_message) = blocks_subscription.read().await?;
            let new_epoch = match blocks_message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::BlockAvailable(_))) => {
                    // Handle rollbacks on this topic only
                    let mut state = state_mutex.lock().await;
                    if block_info.status == BlockStatus::RolledBack {
                        state.volatile.rollback_before(block_info.number);
                    }

                    current_block = Some(block_info.clone());
                    block_info.new_epoch && block_info.epoch > 0
                }
                Message::Cardano((_, CardanoMessage::Rollback(_))) => {
                    // do nothing, rollbacks are handled on BlockAvailable message
                    false
                }
                _ => false,
            };

            // Read from epoch-boundary messages only when it's a new epoch
            if new_epoch {
                let (_, params_msg) = params_subscription.read_ignoring_rollbacks().await?;
                match params_msg.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::ProtocolParams(params))) => {
                        let span = info_span!(
                            "historical_epochs_state.handle_params",
                            epoch = block_info.epoch
                        );
                        async {
                            Self::check_sync(&current_block, block_info);
                            let mut state = state_mutex.lock().await;
                            if let Some(shelley) = &params.params.shelley {
                                state.volatile.update_k(shelley.security_param);
                            }
                        }
                        .instrument(span)
                        .await;
                    }
                    _ => error!("Unexpected message type: {params_msg:?}"),
                }

                let (_, epoch_activity_msg) =
                    epoch_activity_subscription.read_ignoring_rollbacks().await?;
                match epoch_activity_msg.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::EpochActivity(ea))) => {
                        let span = info_span!(
                            "historical_epochs_state.handle_epoch_activity",
                            epoch = block_info.epoch
                        );
                        async {
                            Self::check_sync(&current_block, block_info);
                            let mut state = state_mutex.lock().await;
                            state.volatile.handle_new_epoch(block_info, ea);
                        }
                        .instrument(span)
                        .await;
                    }
                    _ => error!("Unexpected message type: {epoch_activity_msg:?}"),
                }
            }

            // Prune volatile and persist if needed
            if let Some(current_block) = current_block {
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

    /// Async initialisation
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration

        // Subscription topics
        let blocks_subscribe_topic = config
            .get_string(DEFAULT_BLOCKS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCKS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating blocks subscriber on '{blocks_subscribe_topic}'");

        let epoch_activity_subscribe_topic = config
            .get_string(DEFAULT_EPOCH_ACTIVITY_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCH_ACTIVITY_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating epoch activity subscriber on '{epoch_activity_subscribe_topic}'");

        let params_subscribe_topic = config
            .get_string(DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating parameters subscriber on '{params_subscribe_topic}'");

        // Query topic
        let historical_epochs_query_topic = config
            .get_string(DEFAULT_HISTORICAL_EPOCHS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_HISTORICAL_EPOCHS_QUERY_TOPIC.1.to_string());
        info!("Creating query handler on '{historical_epochs_query_topic}'");

        // Configuration
        let config = HistoricalEpochsStateConfig {
            db_path: config
                .get_string(DEFAULT_HISTORICAL_EPOCHS_STATE_DB_PATH.0)
                .unwrap_or(DEFAULT_HISTORICAL_EPOCHS_STATE_DB_PATH.1.to_string()),
            clear_on_start: config
                .get_bool(DEFAULT_CLEAR_ON_START.0)
                .unwrap_or(DEFAULT_CLEAR_ON_START.1),
        };

        // Initalize state
        let state = State::new(&config)?;
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
        let blocks_subscription = context.subscribe(&blocks_subscribe_topic).await?;
        let epoch_activity_subscription =
            context.subscribe(&epoch_activity_subscribe_topic).await?;
        let params_subscription = context.subscribe(&params_subscribe_topic).await?;

        // Start run task
        context.run(async move {
            Self::run(
                state_mutex,
                blocks_subscription,
                epoch_activity_subscription,
                params_subscription,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
