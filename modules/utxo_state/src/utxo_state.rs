//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use acropolis_common::{
    messages::{
        CardanoMessage, Message, SnapshotMessage, SnapshotStateMessage, StateQuery,
        StateQueryResponse, StateTransitionMessage,
    },
    queries::utxos::{UTxOStateQuery, UTxOStateQueryResponse, DEFAULT_UTXOS_QUERY_TOPIC},
};
use caryatid_sdk::{module, Context};

use acropolis_common::queries::errors::QueryError;
use anyhow::{anyhow, Result};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod state;
use state::{ImmutableUTXOStore, State};

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

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.utxo.deltas";
const DEFAULT_STORE: &str = "memory";
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

/// UTXO state module
#[module(
    message_type(Message),
    name = "utxo-state",
    description = "In-memory UTXO state from UTXO events"
)]
pub struct UTXOState;

impl UTXOState {
    /// Wait for and process snapshot bootstrap messages
    async fn wait_for_bootstrap(
        store: Arc<dyn ImmutableUTXOStore>,
        mut snapshot_subscription: Box<dyn caryatid_sdk::Subscription<Message>>,
    ) -> Result<()> {
        info!("Waiting for UTXO state snapshot bootstrap messages...");

        let mut total_utxos_received = 0u64;
        let mut batch_count = 0u64;

        loop {
            let Ok((_, message)) = snapshot_subscription.read().await else {
                info!("Snapshot subscription closed");
                return Ok(());
            };

            match message.as_ref() {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("UTXO state received Snapshot Startup message");
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(
                    SnapshotStateMessage::UTxOPartialState(utxo_state),
                )) => {
                    let batch_size = utxo_state.utxos.len();
                    batch_count += 1;
                    total_utxos_received += batch_size as u64;

                    if batch_count == 1 {
                        info!(
                            "UTXO state received first UTxO batch with {} UTxOs",
                            batch_size
                        );
                    } else if batch_count % 100 == 0 {
                        info!(
                            "UTXO state received {} batches, {} total UTxOs so far",
                            batch_count, total_utxos_received
                        );
                    }

                    for (key, value) in &utxo_state.utxos {
                        if store.add_utxo(*key, value.clone()).await.is_err() {
                            error!("Failed to add snapshot utxo to state store");
                        }
                    }
                }
                Message::Snapshot(SnapshotMessage::Complete) => {
                    info!(
                        "UTXO state snapshot complete: {} UTxOs in {} batches",
                        total_utxos_received, batch_count
                    );
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let snapshot_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating snapshot subscriber on '{snapshot_topic}'");

        let utxos_query_topic = config
            .get_string(DEFAULT_UTXOS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_UTXOS_QUERY_TOPIC.1.to_string());

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

        // Subscribe for snapshot bootstrap (if using snapshot startup)
        let snapshot_subscription = context.subscribe(&snapshot_topic).await?;

        // Subscribe for UTXO messages
        let state1 = state.clone();
        let mut subscription = context.subscribe(&subscribe_topic).await?;
        context.run(async move {
            // Wait for snapshot bootstrap before processing messages
            if let Err(e) = Self::wait_for_bootstrap(snapshot_store, snapshot_subscription).await {
                error!("Failed to bootstrap UTXO state: {e}");
                return;
            }

            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((block, CardanoMessage::UTXODeltas(deltas_msg))) => {
                        let span = info_span!("utxo_state.handle", block = block.number);
                        async {
                            let mut state = state1.lock().await;
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
                        let mut state = state1.lock().await;
                        state
                            .handle_rollback(message)
                            .await
                            .inspect_err(|e| error!("Rollback handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

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
