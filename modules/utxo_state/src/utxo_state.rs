//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{
    Serialiser,
    messages::Message
};

use anyhow::{Result, anyhow};
use config::Config;
use tracing::{info, error};
use std::sync::Arc;
use tokio::sync::Mutex;

mod state;
use state::{State, ImmutableUTXOStore};

mod volatile_index;
mod address_delta_publisher;
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

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.utxo.deltas";
const DEFAULT_STORE: &str = "memory";
const DEFAULT_DATABASE_PATH: &str = "immutable-utxos";

/// UTXO state module
#[module(
    message_type(Message),
    name = "utxo-state",
    description = "In-memory UTXO state from UTXO events"
)]
pub struct UTXOState;

impl UTXOState
{
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let database_path = config.get_string("database-path")
            .unwrap_or(DEFAULT_DATABASE_PATH.to_string());

        // Create store
        let store_type = config.get_string("store").unwrap_or(DEFAULT_STORE.to_string());
        let store: Arc<dyn ImmutableUTXOStore> = match store_type.as_str() {
            "memory" => {
                info!("Storing immutable UTXOs in memory (standard)");
                Arc::new(InMemoryImmutableUTXOStore::new())
            }
            "dashmap" => {
                info!("Storing immutable UTXOs in memory (DashMap)");
                Arc::new(DashMapImmutableUTXOStore::new())
            }
            "sled" => {
                info!("Storing immutable UTXOs with Sled (sync) on disk ({database_path})");
                Arc::new(SledImmutableUTXOStore::new(database_path)?)
            }
            "sled-async" => {
                info!("Storing immutable UTXOs with Sled (async) on disk ({database_path})");
                Arc::new(SledAsyncImmutableUTXOStore::new(database_path)?)
            }
            "fjall" => {
                info!("Storing immutable UTXOs with Fjall (sync) on disk ({database_path})");
                let store = Arc::new(FjallImmutableUTXOStore::new(database_path)?);
                // optionally configure flush_every
                match config.get_int("flush-every") {
                    Ok(n) => store.set_flush_every(n as usize),
                    _ => {}
                }
                store
            }
            "fjall-async" => {
                info!("Storing immutable UTXOs with Fjall (async) on disk ({database_path})");
                let store = Arc::new(FjallAsyncImmutableUTXOStore::new(database_path)?);
                // optionally configure flush_every
                match config.get_int("flush-every") {
                    Ok(n) => store.set_flush_every(n as usize),
                    _ => {}
                }
                store
            }
            _ => return Err(anyhow!("Unknown store type {store_type}"))
        };
        let mut state = State::new(store);

        // Create address delta publisher and pass it observations
        let publisher = AddressDeltaPublisher::new(context.clone(), config);
        state.register_address_delta_observer(Arc::new(publisher));

        let state = Arc::new(Mutex::new(state));
        let state2 = state.clone();
        let serialiser = Arc::new(Mutex::new(Serialiser::new(state, module_path!(), 0)));
        let serialiser2 = serialiser.clone();

        // Subscribe for UTXO messages
        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let serialiser = serialiser.clone();
            async move {
                match message.as_ref() {
                    Message::UTXODeltas(deltas_msg) => {
                        let mut serialiser = serialiser.lock().await;
                        serialiser.handle_message(deltas_msg.sequence, deltas_msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Ticker to log stats and prune state
        context.clone().message_bus.subscribe("clock.tick", move |message: Arc<Message>| {
            let serialiser = serialiser2.clone();
            let state = state2.clone();

            async move {
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state.lock().await.tick()
                            .await
                            .inspect_err(|e| error!("Tick error: {e}"))
                            .ok();
                        serialiser.lock().await.tick();
                    }
                }
            }
        })?;

        Ok(())
    }
}
