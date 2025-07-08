//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use acropolis_common::{
    messages::{CardanoMessage, Message, RESTResponse},
    rest_helper::handle_rest_with_parameter,
};
use caryatid_sdk::{module, Context, Module};

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
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let rest_topic = config.get_string("rest-topic").unwrap_or(DEFAULT_REST_TOPIC.to_string());
        info!("Creating REST handler on '{rest_topic}'");

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

        // Subscribe for UTXO messages
        let state1 = state.clone();
        let mut subscription = context.subscribe(&subscribe_topic).await?;
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
        let mut subscription = context.subscribe("clock.tick").await?;
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

        // Handle REST requests for utxo.<tx_hash>:<index>
        handle_rest_with_parameter(context.clone(), &rest_topic, move |param| {
            let param = param[0].to_string();
            let state = state.clone();
            async move {
                // Parse "<tx_hash>:<index>"
                let (tx_hash_str, index_str) = match param.split_once(':') {
                    Some((tx, idx)) => (tx, idx),
                    None => {
                        return Ok(RESTResponse::with_text(
                            400,
                            &format!(
                                "Parameter must be in <tx_hash>:<index> format. Provided param: {}",
                                param
                            ),
                        ));
                    }
                };

                // Validate tx_hash and index, look up the UTXO, and return JSON or an error.
                match hex::decode(tx_hash_str) {
                    Ok(tx_hash_bytes) => match index_str.parse::<u64>() {
                        Ok(index) => {
                            let key = state::UTXOKey::new(&tx_hash_bytes, index);
                            match state.lock().await.lookup_utxo(&key).await {
                                Ok(Some(utxo)) => {
                                    // Convert address to bech32 string
                                    let address_text = match utxo.address.to_string() {
                                        Ok(addr) => addr,
                                        Err(e) => {
                                            return Ok(RESTResponse::with_text(
                                                500,
                                                &format!(
                                                    "Failed to convert address to string: {e}"
                                                ),
                                            ));
                                        }
                                    };

                                    let json_response = serde_json::json!({
                                        "address": address_text,
                                        "value": utxo.value,
                                    });

                                    Ok(RESTResponse::with_json(200, &json_response.to_string()))
                                }
                                Ok(None) => Ok(RESTResponse::with_text(
                                    404,
                                    &format!("UTxO not found. Provided UTxO: {}", param),
                                )),
                                Err(error) => Err(anyhow!("{:?}", error)),
                            }
                        }
                        Err(error) => Ok(RESTResponse::with_text(
                            400,
                            &format!("Invalid index: {error}"),
                        )),
                    },
                    Err(error) => Ok(RESTResponse::with_text(
                        400,
                        &format!("Invalid tx_hash: {error}"),
                    )),
                }
            }
        });

        Ok(())
    }
}
