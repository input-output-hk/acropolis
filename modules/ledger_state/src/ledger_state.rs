//! Acropolis ledger state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_messages::Message;
use anyhow::Result;
use config::Config;
use tracing::{info, error};
use hex::encode;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.utxo.#";

/// Key of ledger state store
#[derive(Debug, Clone, Eq)]
struct UTXOKey {
    hash: [u8; 32], // Tx hash
    index: u32,     // Output index in the transaction
}

impl UTXOKey {
    /// Creates a new UTXOKey from any slice (pads with zeros if < 32 bytes)
    pub fn new(hash_slice: &[u8], index: u32) -> Self {
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
                    Message::Input(input_msg) => {
                        info!("UTXO << {}:{}", encode(&input_msg.ref_hash), input_msg.ref_index);
                        let key = UTXOKey::new(&input_msg.ref_hash, input_msg.index);
                        let mut state = state.write().unwrap();
                        match state.utxos.remove(&key) {
                            Some(previous) => info!("        - spent {} from {}", previous.value,
                                                    encode(previous.address)),
                            None => info!("        - not previously seen")
                        }
                    }

                    Message::Output(output_msg) => {
                        info!("UTXO >> {}:{}", encode(&output_msg.tx_hash), output_msg.index);
                        info!("        - adding {} to {}", output_msg.value,
                              encode(&output_msg.address));
                        let key = UTXOKey::new(&output_msg.tx_hash, output_msg.index);
                        let mut state = state.write().unwrap();
                        state.utxos.insert(key, UTXOValue { address: output_msg.address.clone(),
                                                            value: output_msg.value });
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        Ok(())
    }
}
