//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_messages::{BlockStatus, Message, UTXODelta};
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};
use hex::encode;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.utxo.deltas";

/// Key of ledger state store
#[derive(Debug, Clone, Eq)]
struct UTXOKey {
    hash: [u8; 32], // Tx hash
    index: u64,     // Output index in the transaction
}

impl UTXOKey {
    /// Creates a new UTXOKey from any slice (pads with zeros if < 32 bytes)
    pub fn new(hash_slice: &[u8], index: u64) -> Self {
        let mut hash = [0u8; 32]; // Initialize with zeros
        let len = hash_slice.len().min(32); // Cap at 32 bytes
        hash[..len].copy_from_slice(&hash_slice[..len]); // Copy input hash
        Self { hash, index }
    }
}

impl PartialEq for UTXOKey {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.hash == other.hash
    }
}

impl Hash for UTXOKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
        self.index.hash(state);
    }
}

/// Value stored in UTXO
#[derive(Debug, Clone)]
struct UTXOValue {
    address: Vec<u8>, // Address
    value: u64,       // Value in Lovelace
}

/// Ledger state storage
struct State {
    utxos: HashMap<UTXOKey, UTXOValue>,    //< Live UTXOs
    future_spends: HashMap<UTXOKey, u64>,  //< UTXOs spent in blocks arriving out of order, to slot
    max_slot: u64,                         //< Maximum block slot received
    max_number: u64,                       //< Maximum block number received
}

impl State {
    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            utxos: HashMap::new(),
            future_spends: HashMap::new(),
            max_slot: 0,
            max_number: 0,
        }
    }
}

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

                        { // Capture maximum slot received
                            let mut state = state.write().unwrap();
                            state.max_slot = state.max_slot.max(deltas_msg.block.slot);
                            state.max_number = state.max_number.max(deltas_msg.block.number);
                        }

                        if let BlockStatus::RolledBack = deltas_msg.block.status {
                            error!(slot = deltas_msg.block.slot,
                                number = deltas_msg.block.number,
                                "Rollback received - we don't handle this yet!")
                        }

                        for delta in &deltas_msg.deltas {  // UTXODelta
                            match delta {
                                UTXODelta::Input(tx_input) => {
                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!("UTXO << {}:{}", encode(&tx_input.tx_hash),
                                              tx_input.index);
                                    }
                                    let key = UTXOKey::new(&tx_input.tx_hash, tx_input.index);
                                    let mut state = state.write().unwrap();
                                    match state.utxos.remove(&key) {
                                        Some(previous) => {
                                            if tracing::enabled!(tracing::Level::DEBUG) {
                                                debug!("        - spent {} from {}",
                                                       previous.value, encode(previous.address));
                                            }
                                        }
                                        None => {
                                            if tracing::enabled!(tracing::Level::DEBUG) {
                                                debug!("UTXO {}:{} arrived out of order (slot {})",
                                                    encode(&tx_input.tx_hash), tx_input.index,
                                                    deltas_msg.block.slot);
                                            }
                                            // Add to future spend set
                                            state.future_spends.insert(key, deltas_msg.block.slot);
                                        }
                                    }
                                },

                                UTXODelta::Output(tx_output) => {
                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!("UTXO >> {}:{}", encode(&tx_output.tx_hash),
                                               tx_output.index);
                                        debug!("        - adding {} to {}", tx_output.value,
                                            encode(&tx_output.address));
                                    }
                                    let key = UTXOKey::new(&tx_output.tx_hash, tx_output.index);
                                    let mut state = state.write().unwrap();
                                    // Check if it was spent in a future block (that arrived
                                    // out of order)
                                    if let Some(slot) = state.future_spends.get(&key) {
                                        // Net effect is zero, so we ignore it
                                        if tracing::enabled!(tracing::Level::DEBUG) {
                                            debug!("UTXO {}:{} in future spends removed (created in slot {}, spent in slot {})",
                                                  encode(&tx_output.tx_hash), tx_output.index,
                                                  deltas_msg.block.slot, slot);
                                        }
                                        state.future_spends.remove(&key);
                                    } else {
                                        state.utxos.insert(key, UTXOValue {
                                            address: tx_output.address.clone(),
                                            value: tx_output.value
                                        });
                                    }
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
                    let state = state2.write().unwrap();
                    info!(slot = state.max_slot,
                          number = state.max_number,
                          utxos = state.utxos.len(),
                          future_spends = state.future_spends.len());
                    for (key, slot) in &state.future_spends {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!("Future spend: UTXO {}:{} from slot {slot}",
                                encode(key.hash), key.index);
                        }
                        if state.max_slot - slot > 10000 {
                            error!("Future spend UTXO {}:{} from slot {slot} is too old (max slot {})",
                                   encode(key.hash), key.index, state.max_slot);
                        }
                    }
                }
            }

            async {}
        })?;

        Ok(())
    }
}
