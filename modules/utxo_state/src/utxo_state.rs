//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse, StateTransitionMessage},
    queries::utxos::{UTxOStateQuery, UTxOStateQueryResponse, DEFAULT_UTXOS_QUERY_TOPIC},
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
        state: Arc<Mutex<State>>,
        mut utxo_deltas_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        loop {
            let Ok((_, message)) = utxo_deltas_subscription.read().await else {
                return Err(anyhow!("Failed to read UTxO deltas subscription error"));
            };

            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::UTXODeltas(deltas_msg))) => {
                    let span = info_span!("utxo_state.validate", block = block.number);
                    async {
                        let mut state = state.lock().await;
                        if let Err(e) = state.validate(block, deltas_msg).await {
                            error!("Validation failed: block={}, error={e}", block.number);
                        }
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
        let mut state = State::new(store);

        // Create address delta publisher and pass it observations
        let publisher = AddressDeltaPublisher::new(context.clone(), config);
        state.register_address_delta_observer(Arc::new(publisher));

        let state = Arc::new(Mutex::new(state));

        // Subscribers
        let utxo_deltas_subscription = context.subscribe(&utxo_deltas_subscribe_topic).await?;

        let state_run = state.clone();
        context.run(async move {
            Self::run(state_run, utxo_deltas_subscription)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
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
