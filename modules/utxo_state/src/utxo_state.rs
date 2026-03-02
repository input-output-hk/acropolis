//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use acropolis_common::{
    configuration::StartupMode,
    messages::{
        CardanoMessage, Message, SnapshotMessage, SnapshotStateMessage, StateQuery,
        StateQueryResponse, StateTransitionMessage,
    },
    queries::utxos::{UTxOStateQuery, UTxOStateQueryResponse, DEFAULT_UTXOS_QUERY_TOPIC},
    validation::ValidationOutcomes,
    BlockInfo,
};
use caryatid_sdk::{module, Context, Subscription};

use acropolis_common::queries::errors::QueryError;
use anyhow::{anyhow, Result};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod state;
use state::{ImmutableUTXOStore, State};
mod address_delta_mode;
use address_delta_mode::AddressDeltaPublishMode;

#[cfg(test)]
mod test_utils;

mod address_delta_publisher;
mod volatile_index;
use address_delta_publisher::AddressDeltaPublisher;
mod block_totals_publisher;
use block_totals_publisher::BlockTotalsPublisher;
mod in_memory_immutable_utxo_store;
use in_memory_immutable_utxo_store::InMemoryImmutableUTXOStore;
mod dashmap_immutable_utxo_store;
use dashmap_immutable_utxo_store::DashMapImmutableUTXOStore;
mod sled_immutable_utxo_store;
use sled_immutable_utxo_store::SledImmutableUTXOStore;
mod fjall_immutable_utxo_store;
use fjall_immutable_utxo_store::FjallImmutableUTXOStore;
mod fake_immutable_utxo_store;
use fake_immutable_utxo_store::FakeImmutableUTXOStore;
mod utils;
mod validations;

const DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("utxo-deltas-subscribe-topic", "cardano.utxo.deltas");
const DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC: (&str, &str) = (
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
);

const DEFAULT_STORE: &str = "memory";
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");
const DEFAULT_UTXO_VALIDATION_TOPIC: (&str, &str) =
    ("utxo-validation-publish-topic", "cardano.validation.utxo");
const DEFAULT_ADDRESS_DELTA_PUBLISH_MODE: &str = "compact";

/// UTXO state module
#[module(
    message_type(Message),
    name = "utxo-state",
    description = "In-memory UTXO state from UTXO events"
)]
pub struct UTXOState;

impl UTXOState {
    /// Main run function
    #[allow(clippy::too_many_arguments)]
    async fn run(
        context: Arc<Context<Message>>,
        state: Arc<Mutex<State>>,
        mut utxo_deltas_subscription: Box<dyn Subscription<Message>>,
        mut protocol_parameters_subscription: Box<dyn Subscription<Message>>,
        mut pool_registration_updates_subscription: Option<Box<dyn Subscription<Message>>>,
        mut stake_registration_updates_subscription: Option<Box<dyn Subscription<Message>>>,
        publish_tx_validation_topic: String,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        let mut bootstrap_block_processed = false;

        loop {
            let mut current_block_info: Option<BlockInfo> = None;
            let Ok((_, message)) = utxo_deltas_subscription.read().await else {
                return Err(anyhow!("Failed to read UTxO deltas subscription error"));
            };

            let new_epoch = match message.as_ref() {
                Message::Cardano((block_info, _)) => {
                    current_block_info = Some(block_info.clone());
                    block_info.new_epoch
                }

                _ => false,
            };

            let mut current_protocol_params = state.lock().await.get_or_init_protocol_parameters();

            // Read protocol parameters if new epoch
            if new_epoch {
                let (_, protocol_parameters_message) =
                    protocol_parameters_subscription.read().await?;

                if let Message::Cardano((_, CardanoMessage::ProtocolParams(params))) =
                    protocol_parameters_message.as_ref()
                {
                    current_protocol_params = params.params.clone();
                }
            }

            // Read from pool registration updates subscription if available
            let mut pool_registration_updates = vec![];
            if is_snapshot_mode || bootstrap_block_processed {
                if let Some(subscription) = pool_registration_updates_subscription.as_mut() {
                    let Ok((_, message)) = subscription.read().await else {
                        error!("Failed to read pool registration updates subscription error");
                        continue;
                    };
                    if let Message::Cardano((
                        block_info,
                        CardanoMessage::PoolRegistrationUpdates(updates_msg),
                    )) = message.as_ref()
                    {
                        Self::check_sync(&current_block_info, block_info);
                        pool_registration_updates = updates_msg.updates.clone();
                    }
                }
            }

            // Read from stake registration updates subscription if available
            let mut stake_registration_updates = vec![];
            if is_snapshot_mode || bootstrap_block_processed {
                if let Some(subscription) = stake_registration_updates_subscription.as_mut() {
                    let Ok((_, message)) = subscription.read().await else {
                        error!("Failed to read stake registration updates subscription error");
                        continue;
                    };
                    if let Message::Cardano((
                        block_info,
                        CardanoMessage::StakeRegistrationUpdates(updates_msg),
                    )) = message.as_ref()
                    {
                        Self::check_sync(&current_block_info, block_info);
                        stake_registration_updates = updates_msg.updates.clone();
                    }
                }
            }

            // Validate UTxODeltas
            // before applying them
            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::UTXODeltas(deltas_msg))) => {
                    let span = info_span!("utxo_state.validate", block = block.number);
                    async {
                        let mut state = state.lock().await;
                        let mut validation_outcomes = ValidationOutcomes::new();
                        if let Err(e) = state
                            .validate(
                                block,
                                deltas_msg,
                                &pool_registration_updates,
                                &stake_registration_updates,
                                &current_protocol_params,
                            )
                            .await
                        {
                            validation_outcomes.push(*e);
                        }

                        validation_outcomes
                            .publish(&context, "utxo_state", &publish_tx_validation_topic, block)
                            .await
                            .unwrap_or_else(|e| error!("Failed to publish UTxO validation: {e}"));
                    }
                    .instrument(span)
                    .await;

                    let span = info_span!("utxo_state.handle", block = block.number);
                    async {
                        let mut state = state.lock().await;
                        state
                            .handle_utxo_deltas(block, deltas_msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }
                    .instrument(span)
                    .await;

                    if !bootstrap_block_processed {
                        bootstrap_block_processed = true;
                    }
                }

                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    let mut state = state.lock().await;
                    state
                        .handle_rollback(message)
                        .await
                        .inspect_err(|e| error!("Rollback handling error: {e}"))
                        .ok();
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Commit protocol paramemters
            if let Some(block_info) = current_block_info {
                state
                    .lock()
                    .await
                    .commit_protocol_parameters(&block_info, current_protocol_params.clone());
            }
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let utxo_deltas_subscribe_topic = config
            .get_string(DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{utxo_deltas_subscribe_topic}'");

        let protocol_parameters_subscribe_topic = config
            .get_string(DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating protocol parameters subscriber on '{protocol_parameters_subscribe_topic}'");

        // These registration updates subscriptions are only needed for validation
        let pool_registration_updates_subscribe_topic =
            config.get_string("pool-registration-updates-subscribe-topic").ok();
        if let Some(ref topic) = pool_registration_updates_subscribe_topic {
            info!("Creating pool registration updates subscriber on '{topic}'");
        }
        let stake_registration_updates_subscribe_topic =
            config.get_string("stake-registration-updates-subscribe-topic").ok();
        if let Some(ref topic) = stake_registration_updates_subscribe_topic {
            info!("Creating stake registration updates subscriber on '{topic}'");
        }

        let snapshot_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating snapshot subscriber on '{snapshot_topic}'");

        let utxos_query_topic = config
            .get_string(DEFAULT_UTXOS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_UTXOS_QUERY_TOPIC.1.to_string());

        let utxo_validation_publish_topic = config
            .get_string(DEFAULT_UTXO_VALIDATION_TOPIC.0)
            .unwrap_or(DEFAULT_UTXO_VALIDATION_TOPIC.1.to_string());
        info!("Creating UTxO validation publisher on '{utxo_validation_publish_topic}'");

        let address_delta_publish_mode = config
            .get_string("address-delta-publish-mode")
            .unwrap_or_else(|_| DEFAULT_ADDRESS_DELTA_PUBLISH_MODE.to_string())
            .parse::<AddressDeltaPublishMode>()?;
        info!(
            mode = ?address_delta_publish_mode,
            "Address delta publish mode"
        );

        let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();

        // Create store
        let store_type = config.get_string("store").unwrap_or(DEFAULT_STORE.to_string());
        let store: Arc<dyn ImmutableUTXOStore> = match store_type.as_str() {
            "memory" => Arc::new(InMemoryImmutableUTXOStore::new(config.clone())),
            "dashmap" => Arc::new(DashMapImmutableUTXOStore::new(config.clone())),
            "sled" => Arc::new(SledImmutableUTXOStore::new(config.clone())?),
            "fjall" => Arc::new(FjallImmutableUTXOStore::new(config.clone())?),
            "fake" => Arc::new(FakeImmutableUTXOStore::new(config.clone())),
            _ => return Err(anyhow!("Unknown store type {store_type}")),
        };
        let snapshot_store = store.clone();
        let mut state = State::new(store, address_delta_publish_mode);

        // Create address delta publisher and pass it observations
        let deltas_publisher =
            AddressDeltaPublisher::new(context.clone(), config.clone(), address_delta_publish_mode);
        state.register_address_delta_observer(Arc::new(deltas_publisher));

        // Create block totals publisher and pass it observations
        let totals_publisher = BlockTotalsPublisher::new(context.clone(), config);
        state.register_block_totals_observer(Arc::new(totals_publisher));

        let state = Arc::new(Mutex::new(state));

        // Subscribers
        let utxo_deltas_subscription = context.subscribe(&utxo_deltas_subscribe_topic).await?;
        let protocol_parameters_subscription =
            context.subscribe(&protocol_parameters_subscribe_topic).await?;
        let pool_registration_updates_subscription =
            if let Some(topic) = pool_registration_updates_subscribe_topic {
                Some(context.subscribe(&topic).await?)
            } else {
                None
            };
        let stake_registration_updates_subscription =
            if let Some(topic) = stake_registration_updates_subscribe_topic {
                Some(context.subscribe(&topic).await?)
            } else {
                None
            };

        let state_run = state.clone();
        let context_run = context.clone();
        context.run(async move {
            Self::run(
                context_run,
                state_run,
                utxo_deltas_subscription,
                protocol_parameters_subscription,
                pool_registration_updates_subscription,
                stake_registration_updates_subscription,
                utxo_validation_publish_topic,
                is_snapshot_mode,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        // Subscribe for snapshot messages
        {
            let mut subscription = context.subscribe(&snapshot_topic).await?;
            let context = context.clone();
            let store = snapshot_store.clone();
            enum SnapshotState {
                Preparing,
                Started,
            }
            let mut snapshot_state = SnapshotState::Preparing;
            let mut total_utxos_received = 0u64;
            let mut batch_count = 0u64;
            context.run(async move {
                loop {
                    let Ok((_, message)) = subscription.read().await else {
                        return;
                    };

                    match message.as_ref() {
                        Message::Snapshot(SnapshotMessage::Startup) => {
                            info!("UTXO state received Snapshot Startup message");
                            match snapshot_state {
                                SnapshotState::Preparing => snapshot_state = SnapshotState::Started,
                                _ => error!("Snapshot Startup message received but we have already left preparing state"),
                            }
                        }
                        Message::Snapshot(SnapshotMessage::Bootstrap(
                            SnapshotStateMessage::UTxOPartialState(utxo_state),
                        )) => {
                            let batch_size = utxo_state.utxos.len();
                            batch_count += 1;
                            total_utxos_received += batch_size as u64;

                            if batch_count == 1 {
                                info!("UTXO state received first UTxO batch with {} UTxOs", batch_size);
                            } else if batch_count.is_multiple_of(100) {
                                info!("UTXO state received {} batches, {} total UTxOs so far", batch_count, total_utxos_received);
                            }

                            for (key, value) in &utxo_state.utxos {
                                if store.add_utxo(*key, value.clone()).await.is_err() {
                                    error!("Failed to add snapshot utxo to state store");
                                }
                            }
                        }
                        Message::Snapshot(SnapshotMessage::Complete) => {
                            info!("UTXO state snapshot complete: {} UTxOs in {} batches", total_utxos_received, batch_count);
                            return;
                        }
                        _ => {}
                    }
                }
            });
        }

        // Query handler
        let state_query = state.clone();
        context.handle(&utxos_query_topic, move |message| {
            let state_mutex = state_query.clone();
            async move {
                let Message::StateQuery(StateQuery::UTxOs(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::UTxOs(
                        UTxOStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for utxo-state",
                        )),
                    )));
                };

                let mut state = state_mutex.lock().await;
                let response = match query {
                    UTxOStateQuery::GetUTxOsSum { utxo_identifiers } => {
                        match state.get_utxos_sum(utxo_identifiers).await {
                            Ok(balance) => UTxOStateQueryResponse::UTxOsSum(balance),
                            Err(e) => UTxOStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    UTxOStateQuery::GetUTxOs { utxo_identifiers } => {
                        match state.get_utxo_entries(utxo_identifiers).await {
                            Ok(values) => UTxOStateQueryResponse::UTxOs(values),
                            Err(e) => UTxOStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    UTxOStateQuery::GetAllUTxOsSumAtShelleyStart => {
                        let total_lovelace = match state.get_lovelace_at_shelley_start() {
                            Some(cached) => cached,
                            None => match state.get_total_lovelace().await {
                                Ok(v) => v,
                                Err(e) => {
                                    return Arc::new(Message::StateQueryResponse(
                                        StateQueryResponse::UTxOs(UTxOStateQueryResponse::Error(
                                            QueryError::internal_error(e.to_string()),
                                        )),
                                    ));
                                }
                            },
                        };
                        UTxOStateQueryResponse::LovelaceSum(total_lovelace)
                    }
                    UTxOStateQuery::GetAvvmCancelledValue => {
                        if state.get_avvm_cancelled_value().is_none() {
                            if let Err(e) = state.cancel_redeem_utxos().await {
                                error!("Failed to cancel AVVM UTxOs on query: {e}");
                            }
                        }
                        UTxOStateQueryResponse::AvvmCancelledValue(state.get_avvm_cancelled_value())
                    }
                    UTxOStateQuery::GetPointerAddressValues => {
                        if state.get_pointer_address_values().is_none() {
                            if let Err(e) = state.compute_pointer_address_values().await {
                                error!("Failed to compute pointer address values: {e}");
                            }
                        }
                        match state.get_pointer_address_values() {
                            Some(values) => {
                                UTxOStateQueryResponse::PointerAddressValues(values.clone())
                            }
                            None => UTxOStateQueryResponse::PointerAddressValues(
                                std::collections::HashMap::new(),
                            ),
                        }
                    }
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::UTxOs(
                    response,
                )))
            }
        });

        // Ticker to log stats and prune state
        let state2 = state.clone();
        let mut subscription = context.subscribe("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("utxo_state.tick", number = message.number);
                        async {
                            state2
                                .lock()
                                .await
                                .tick()
                                .await
                                .inspect_err(|e| error!("Tick error: {e}"))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }
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
