//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{
    Serialiser,
    messages::Message
};

use anyhow::Result;
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
mod sled_immutable_utxo_store;
use sled_immutable_utxo_store::SledImmutableUTXOStore;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.utxo.deltas";

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

        // Create store - Sled on disk if database-path is set, in-memory otherwise
        let store: Arc<dyn ImmutableUTXOStore> = match config.get_string("database-path") {
            Ok(path) => {
                info!("Storing immutable UTXOs on disk ({path})");
                Arc::new(SledImmutableUTXOStore::new(path)?)
            }
            _ => {
                info!("Storing immutable UTXOs in memory");
                Arc::new(InMemoryImmutableUTXOStore::new())
            }
        };
        let mut state = State::new(store);

        // Create address delta publisher and pass it observations
        let publisher = AddressDeltaPublisher::new(context.clone(), config);
        state.register_address_delta_observer(Arc::new(publisher));

        let state = Arc::new(Mutex::new(state));
        let state2 = state.clone();
        let serialiser = Arc::new(Mutex::new(Serialiser::new(state, module_path!())));
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
