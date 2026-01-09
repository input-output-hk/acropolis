//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use acropolis_common::{
    messages::{
        CardanoMessage, Message, SnapshotMessage, SnapshotStateMessage, StateQuery,
        StateQueryResponse, StateTransitionMessage,
    },
    queries::utxos::{UTxOStateQuery, UTxOStateQueryResponse, DEFAULT_UTXOS_QUERY_TOPIC},
    validation::ValidationOutcomes,
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

#[cfg(test)]
mod test_utils;

mod address_delta_publisher;
mod volatile_index;
use address_delta_publisher::AddressDeltaPublisher;
mod in_memory_immutable_utxo_store;
use in_memory_immutable_utxo_store::InMemoryImmutableUTXOStore;
mod dashmap_immutable_utxo_store;
use dashmap_immutable_utxo_store::DashMapImmutableUTXOStore;
mod sled_immutable_utxo_store;
use sled_immutable_utxo_store::SledImmutableUTXOStore;
mod sled_async_immutable_utxo_store;
use sled_async_immutable_utxo_store::SledAsyncImmutableUTXOStore;
mod fjall_immutable_utxo_store;
use fjall_immutable_utxo_store::FjallImmutableUTXOStore;
mod fjall_async_immutable_utxo_store;
use fjall_async_immutable_utxo_store::FjallAsyncImmutableUTXOStore;
mod fake_immutable_utxo_store;
use fake_immutable_utxo_store::FakeImmutableUTXOStore;

mod validations;

const DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("utxo-deltas-subscribe-topic", "cardano.utxo.deltas");
const DEFAULT_STORE: &str = "memory";
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");
const DEFAULT_UTXO_VALIDATION_TOPIC: (&str, &str) =
    ("utxo-validation-publish-topic", "cardano.validation.utxo");

/// UTXO state module
#[module(
    message_type(Message),
    name = "utxo-state",
    description = "In-memory UTXO state from UTXO events"
)]
pub struct UTXOState;

impl UTXOState {
    /// Main run function
    async fn run(
        context: Arc<Context<Message>>,
        state: Arc<Mutex<State>>,
        mut utxo_deltas_subscription: Box<dyn Subscription<Message>>,
        publish_tx_validation_topic: String,
    ) -> Result<()> {
        loop {
            let Ok((_, message)) = utxo_deltas_subscription.read().await else {
                return Err(anyhow!("Failed to read UTxO deltas subscription error"));
            };

            // Validate UTxODeltas
            // before applying them
            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::UTXODeltas(deltas_msg))) => {
                    let span = info_span!("utxo_state.validate", block = block.number);
                    async {
                        let mut state = state.lock().await;
                        let mut validation_outcomes = ValidationOutcomes::new();
                        if let Err(e) = state.validate(block, deltas_msg).await {
                            validation_outcomes.push(*e);
                        }

                        validation_outcomes
                            .publish(&context, &publish_tx_validation_topic, block)
                            .await
                            .unwrap_or_else(|e| error!("Failed to publish UTxO validation: {e}"));
                    }
                    .instrument(span)
                    .await;

                    let span = info_span!("utxo_state.handle", block = block.number);
                    async {
                        let mut state = state.lock().await;
                        state
                            .handle(block, deltas_msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }
                    .instrument(span)
                    .await;
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
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let utxo_deltas_subscribe_topic = config
            .get_string(DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{utxo_deltas_subscribe_topic}'");

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

        // Create store
        let store_type = config.get_string("store").unwrap_or(DEFAULT_STORE.to_string());
        let store: Arc<dyn ImmutableUTXOStore> = match store_type.as_str() {
            "memory" => Arc::new(InMemoryImmutableUTXOStore::new(config.clone())),
            "dashmap" => Arc::new(DashMapImmutableUTXOStore::new(config.clone())),
            "sled" => Arc::new(SledImmutableUTXOStore::new(config.clone())?),
            "sled-async" => Arc::new(SledAsyncImmutableUTXOStore::new(config.clone())?),
            "fjall" => Arc::new(FjallImmutableUTXOStore::new(config.clone())?),
            "fjall-async" => Arc::new(FjallAsyncImmutableUTXOStore::new(config.clone())?),
            "fake" => Arc::new(FakeImmutableUTXOStore::new(config.clone())),
            _ => return Err(anyhow!("Unknown store type {store_type}")),
        };
        let snapshot_store = store.clone();
        let mut state = State::new(store);

        // Create address delta publisher and pass it observations
        let publisher = AddressDeltaPublisher::new(context.clone(), config);
        state.register_address_delta_observer(Arc::new(publisher));

        let state = Arc::new(Mutex::new(state));

        // Subscribers
        let utxo_deltas_subscription = context.subscribe(&utxo_deltas_subscribe_topic).await?;

        let state_run = state.clone();
        let context_run = context.clone();
        context.run(async move {
            Self::run(
                context_run,
                state_run,
                utxo_deltas_subscription,
                utxo_validation_publish_topic,
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

                let state = state_mutex.lock().await;
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
}
