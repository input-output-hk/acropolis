//! Acropolis ledger state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_messages::{UTXODelta, Message};
use anyhow::Result;
use config::Config;
use tracing::{info, error};
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
    utxos: HashMap<UTXOKey, UTXOValue>,
}

impl State {
    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            utxos: HashMap::new(),
        }
    }
}

/// Ledger state module
#[module(
    message_type(Message),
    name = "ledger-state",
    description = "In-memory ledger state from UTXO events"
)]
pub struct LedgerState;

impl LedgerState
{
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        let state = Arc::new(RwLock::new(State::new()));

        // Subscribe for UTXO messages
        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {

            let state = state.clone();

            async move {
                match message.as_ref() {
                    Message::UTXODeltas(deltas_msg) => {

                        info!("Received {} deltas for slot {}", deltas_msg.deltas.len(),
                              deltas_msg.slot);

                        for delta in &deltas_msg.deltas {  // UTXODelta
                            match delta {
                                UTXODelta::Input(tx_input) => {
                                    info!("UTXO << {}:{}", encode(&tx_input.tx_hash),
                                          tx_input.index);
                                    let key = UTXOKey::new(&tx_input.tx_hash, tx_input.index);
                                    let mut state = state.write().unwrap();
                                    match state.utxos.remove(&key) {
                                        Some(previous) => info!("        - spent {} from {}",
                                                                previous.value,
                                                                encode(previous.address)),
                                        None => error!("UTXO {}:{} not previously seen",
                                                       encode(&tx_input.tx_hash), tx_input.index),
                                    }
                                },

                                UTXODelta::Output(tx_output) => {
                                    info!("UTXO >> {}:{}", encode(&tx_output.tx_hash),
                                          tx_output.index);
                                    info!("        - adding {} to {}", tx_output.value,
                                          encode(&tx_output.address));
                                    let key = UTXOKey::new(&tx_output.tx_hash, tx_output.index);
                                    let mut state = state.write().unwrap();
                                    state.utxos.insert(key, UTXOValue {
                                        address: tx_output.address.clone(),
                                        value: tx_output.value
                                    });
                                },

                                _ => {}
                            }
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        Ok(())
    }
}
