//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use acropolis_common::messages::{CardanoMessage, Message, RESTResponse};
use caryatid_sdk::{module, Context, MessageBusExt, Module};

use anyhow::{anyhow, Result};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

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
const DEFAULT_REST_TOPIC: &str = "rest.get.utxo.*";

const DEFAULT_STORE: &str = "memory";

/// UTXO state module
#[module(
    message_type(Message),
    name = "utxo-state",
    description = "In-memory UTXO state from UTXO events"
)]
pub struct UTXOState;

impl UTXOState {
    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic = config
            .get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let rest_topic = config
            .get_string("rest-topic")
            .unwrap_or(DEFAULT_REST_TOPIC.to_string());
        info!("Creating REST handler on '{rest_topic}'");

        // Create store
        let store_type = config
            .get_string("store")
            .unwrap_or(DEFAULT_STORE.to_string());
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

        // Subscribe for UTXO messages
        let state1 = state.clone();
        let mut subscription = context.message_bus.register(&subscribe_topic).await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((block, CardanoMessage::UTXODeltas(deltas_msg))) => {
                        let mut state = state1.lock().await;
                        state
                            .handle(block, deltas_msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        // Ticker to log stats and prune state
        let state2 = state.clone();
        let mut subscription = context.message_bus.register("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state2
                            .lock()
                            .await
                            .tick()
                            .await
                            .inspect_err(|e| error!("Tick error: {e}"))
                            .ok();
                    }
                }
            }
        });

        // Handle REST requests for utxo.<id>
        let state3 = state.clone();
        context
            .message_bus
            .handle(&rest_topic, move |message: Arc<Message>| {
                let state = state3.clone();

                async move {
                    let response =
                        match message.as_ref() {
                            Message::RESTRequest(request) => {
                                info!("REST received {} {}", request.method, request.path);
                                match request.path_elements.get(1) {
                            Some(id) => match hex::decode(&id) {
                                Ok(id) => {
                                    let key = state::UTXOKey::new(&id, 0); // TODO parse :index
                                    match state.lock().await.lookup_utxo(&key).await {
                                        Ok(Some(utxo)) => match serde_json::to_string(&utxo) {
                                            Ok(body) => RESTResponse::with_json(200, &body),
                                            Err(error) => RESTResponse::with_text(500,
                                                &format!("{error:?}").to_string()),
                                        },
                                        _ => RESTResponse::with_text(404, "UTXO not found"),
                                    }
                                },
                                Err(error) => RESTResponse::with_text(400,
                                    &format!("UTXO must be hex encoded vector of bytes: {error:?}") // TODO real format
                                    .to_string()),
                            },
                            None => RESTResponse::with_text(400, "UTXO id must be provided"),
                        }
                            }
                            _ => {
                                error!("Unexpected message type {:?}", message);
                                RESTResponse::with_text(500, "Unexpected message in REST request")
                            }
                        };

                    Arc::new(Message::RESTResponse(response))
                }
            })?;

        Ok(())
    }
}
