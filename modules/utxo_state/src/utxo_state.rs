//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use acropolis_common::{
    configuration::StartupMethod,
    messages::{
        CardanoMessage, Message, SnapshotMessage, SnapshotStateMessage, StateQuery,
        StateQueryResponse, StateTransitionMessage,
    },
    queries::utxos::{UTxOStateQuery, UTxOStateQueryResponse, DEFAULT_UTXOS_QUERY_TOPIC},
};
use caryatid_sdk::{message_bus::Subscription, module, Context};

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
const DEFAULT_SNAPSHOT_COMPLETION_TOPIC: (&str, &str) =
    ("snapshot-completion-topic", "cardano.snapshot.complete");

/// UTXO state module
#[module(
    message_type(Message),
    name = "utxo-state",
    description = "In-memory UTXO state from UTXO events"
)]
pub struct UTXOState;

impl UTXOState {
    /// Wait for and process snapshot bootstrap messages
    /// Blocks until all UTxO batches are received and UTxOBootstrapComplete is received,
    /// then waits for SnapshotComplete to get block info
    /// Returns the block info (slot, number) from the SnapshotComplete message
    async fn wait_for_bootstrap(
        store: Arc<dyn ImmutableUTXOStore>,
        mut snapshot_subscription: Box<dyn Subscription<Message>>,
        mut completion_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<(u64, u64)> {
        info!("UTxO state: Waiting for snapshot bootstrap messages...");

        let mut total_utxos_received = 0u64;
        let mut batch_count = 0u64;

        // Process UTxO batches from snapshot topic until we receive UTxOBootstrapComplete
        loop {
            let Ok((_, message)) = snapshot_subscription.read().await else {
                return Err(anyhow!("Snapshot subscription closed before completion"));
            };

            match message.as_ref() {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("UTxO state: Received Startup signal, awaiting bootstrap data...");
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(
                    SnapshotStateMessage::UTxOPartialState(utxo_state),
                )) => {
                    let batch_size = utxo_state.utxos.len();
                    batch_count += 1;
                    total_utxos_received += batch_size as u64;

                    if batch_count == 1 {
                        info!(
                            "UTxO state: Received first UTxO batch with {} UTxOs",
                            batch_size
                        );
                    } else if batch_count.is_multiple_of(100) {
                        info!(
                            "UTxO state: Received {} batches, {} total UTxOs so far",
                            batch_count, total_utxos_received
                        );
                    }

                    for (key, value) in &utxo_state.utxos {
                        if store.add_utxo(*key, value.clone()).await.is_err() {
                            error!("UTxO state: Failed to add snapshot utxo to state store");
                        }
                    }
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(
                    SnapshotStateMessage::UTxOBootstrapComplete(complete),
                )) => {
                    info!(
                        "UTxO state: Received UTxOBootstrapComplete - {} UTxOs in {} batches",
                        complete.total_utxos, complete.batch_count
                    );
                    break;
                }
                _ => {
                    // Ignore other snapshot messages (e.g., DRepState, AccountsState, etc.)
                }
            }
        }

        // Wait for SnapshotComplete to get block info
        info!("UTxO state: Waiting for SnapshotComplete message...");
        let (_, message) = completion_subscription.read().await?;
        match message.as_ref() {
            Message::Cardano((block_info, CardanoMessage::SnapshotComplete)) => {
                info!(
                    "UTxO state: Bootstrap complete at block {} slot {} epoch {}",
                    block_info.number, block_info.slot, block_info.epoch
                );
                Ok((block_info.slot, block_info.number))
            }
            other => {
                error!(
                    "UTxO state: Unexpected message on completion topic: {:?}",
                    std::any::type_name_of_val(other)
                );
                Err(anyhow!(
                    "Unexpected message on completion topic: {:?}",
                    other
                ))
            }
        }
    }

    /// Async run loop for processing UTXO delta messages
    async fn run(
        state: Arc<Mutex<State>>,
        store: Arc<dyn ImmutableUTXOStore>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        completion_subscription: Option<Box<dyn Subscription<Message>>>,
        mut deltas_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        // Wait for snapshot bootstrap first (if configured)
        if let (Some(snapshot_sub), Some(completion_sub)) =
            (snapshot_subscription, completion_subscription)
        {
            let (slot, number) =
                Self::wait_for_bootstrap(store, snapshot_sub, completion_sub).await?;
            // Update state with block info from snapshot
            state.lock().await.set_block_info(slot, number);
        }

        info!("UTxO state: Starting main message loop");

        // Main message loop for UTXO deltas
        loop {
            let Ok((_, message)) = deltas_subscription.read().await else {
                info!("UTxO state: Deltas subscription closed, exiting run loop");
                return Ok(());
            };

            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::UTXODeltas(deltas_msg))) => {
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
                        .handle_rollback(message.clone())
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
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let snapshot_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());

        let snapshot_completion_topic = config
            .get_string(DEFAULT_SNAPSHOT_COMPLETION_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_COMPLETION_TOPIC.1.to_string());

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

        let mut state = State::new(store.clone());

        // Create address delta publisher and pass it observations
        let publisher = AddressDeltaPublisher::new(context.clone(), config.clone());
        state.register_address_delta_observer(Arc::new(publisher));

        let state = Arc::new(Mutex::new(state));

        // Only subscribe to snapshot if startup method is snapshot
        let (snapshot_subscription, completion_subscription) =
            if StartupMethod::from_config(config.as_ref()).is_snapshot() {
                info!("Creating snapshot subscriber on '{snapshot_topic}'");
                info!("Creating snapshot completion subscriber on '{snapshot_completion_topic}'");
                (
                    Some(context.subscribe(&snapshot_topic).await?),
                    Some(context.subscribe(&snapshot_completion_topic).await?),
                )
            } else {
                info!("Skipping snapshot subscription (startup method is not snapshot)");
                (None, None)
            };

        // Subscribe for UTXO delta messages
        let deltas_subscription = context.subscribe(&subscribe_topic).await?;

        // Run the main loop (handles bootstrap then deltas)
        let run_state = state.clone();
        let run_store = store.clone();
        context.run(async move {
            Self::run(
                run_state,
                run_store,
                snapshot_subscription,
                completion_subscription,
                deltas_subscription,
            )
            .await
            .inspect_err(|e| error!("UTxO state run error: {e}"))
            .ok();
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
