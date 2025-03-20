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
use std::sync::{Arc, Mutex};

mod state;
use state::State;

mod volatile_index;
mod address_delta_publisher;
use address_delta_publisher::AddressDeltaPublisher;

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

        let mut state = State::new();

        // Create address delta publisher and pass it observations
        let publisher = AddressDeltaPublisher::new(context.clone(), config);
        state.register_address_delta_observer(Arc::new(Mutex::new(publisher)));

        let state = Arc::new(Mutex::new(state));
        let state2 = state.clone();
        let serialiser = Arc::new(Mutex::new(Serialiser::new(state)));
        let serialiser2 = serialiser.clone();

        // Subscribe for UTXO messages
        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let serialiser = serialiser.clone();
            async move {
                match message.as_ref() {
                    Message::UTXODeltas(deltas_msg) => {
                        let mut serialiser = serialiser.lock().unwrap();
                        serialiser.handle_message(&deltas_msg.block, deltas_msg);
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Ticker to log stats and prune state
        context.clone().message_bus.subscribe("clock.tick", move |message: Arc<Message>| {
            if let Message::Clock(message) = message.as_ref() {
                if (message.number % 60) == 0 {
                    state2.lock().unwrap().tick();
                    serialiser2.lock().unwrap().tick();
                }
            }

            async {}
        })?;

        Ok(())
    }
}
