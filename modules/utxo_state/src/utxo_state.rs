//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_messages::{Message, UTXODelta};
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};
use std::sync::{Arc, RwLock};

mod state;
use state::State;

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

        let state = Arc::new(RwLock::new(State::new()));

        // Subscribe for UTXO messages
        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let state2 = state.clone();
        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let state = state.clone();
            async move {
                match message.as_ref() {
                    Message::UTXODeltas(deltas_msg) => {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!("Received {} deltas for slot {}", deltas_msg.deltas.len(),
                                  deltas_msg.block.slot);
                        }

                        { // Capture maximum slot received and handle rollbacks
                            let mut state = state.write().unwrap();
                            state.observe_block(&deltas_msg.block);
                        }

                        for delta in &deltas_msg.deltas {  // UTXODelta
                            let slot = deltas_msg.block.slot;
                            let mut state = state.write().unwrap();

                            match delta {
                                UTXODelta::Input(tx_input) => {
                                    state.observe_input(&tx_input, slot);
                                },

                                UTXODelta::Output(tx_output) => {
                                    state.observe_output(&tx_output, slot);
                                },

                                _ => {}
                            }
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Ticker to log stats
        context.clone().message_bus.subscribe("clock.tick", move |message: Arc<Message>| {
            if let Message::Clock(message) = message.as_ref() {
                if (message.number % 60) == 0 {
                    let state = state2.read().unwrap();
                    state.log_stats();
                }
            }

            async {}
        })?;

        Ok(())
    }
}
