//! Acropolis UTXOState: State storage
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use acropolis_messages::{BlockInfo, BlockStatus, TxInput, TxOutput};
use tracing::{debug, info, error};
use hex::encode;

/// Key of ledger state store
#[derive(Debug, Clone, Eq)]
pub struct UTXOKey {
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
pub struct UTXOValue {
    address: Vec<u8>, // Address
    value: u64,       // Value in Lovelace
}

/// Ledger state storage
pub struct State {
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

    /// Observe a block for statistics and handle rollbacks
    pub fn observe_block(&mut self, block: &BlockInfo) {
        self.max_slot = self.max_slot.max(block.slot);
        self.max_number = self.max_number.max(block.number);

        if matches!(block.status, BlockStatus::RolledBack) {
            error!(slot = block.slot, number = block.number,
                "Rollback received - we don't handle this yet!")
        }
    }

    /// Observe an input UTXO spend
    pub fn observe_input(&mut self, input: &TxInput, slot: u64) {
        let key = UTXOKey::new(&input.tx_hash, input.index);
        
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO << {}:{}", encode(&key.hash), key.index);
        }

        match self.utxos.remove(&key) {
            Some(previous) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("        - spent {} from {}",
                           previous.value, encode(previous.address));
                }
            }
            _ => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("UTXO {}:{} arrived out of order (slot {})",
                           encode(&key.hash), key.index, slot);
                }
                // Add to future spend set
                self.future_spends.insert(key, slot);
            }
        }
    } 

    /// Observe an output UXTO creation
    pub fn observe_output(&mut self,  output: &TxOutput, slot: u64) {

        let key = UTXOKey::new(&output.tx_hash, output.index);

        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO >> {}:{}", encode(&key.hash), key.index);
            debug!("        - adding {} to {}", output.value, encode(&output.address));
        }

        // Check if it was spent in a future block (that arrived
        // out of order)
        if let Some(old_slot) = self.future_spends.get(&key) {
            // Net effect is zero, so we ignore it
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("UTXO {}:{} in future spends removed (created in slot {}, spent in slot {})",
                        encode(&key.hash), key.index, slot, old_slot);
            }
            self.future_spends.remove(&key);
        } else {
            self.utxos.insert(key, UTXOValue {
                address: output.address.clone(),
                value: output.value
            });
        }
    }

    /// Log statistics
    pub fn log_stats(&self) {
        info!(slot = self.max_slot,
            number = self.max_number,
            utxos = self.utxos.len(),
            future_spends = self.future_spends.len());
      for (key, slot) in &self.future_spends {
          if tracing::enabled!(tracing::Level::DEBUG) {
              debug!("Future spend: UTXO {}:{} from slot {slot}",
                  encode(key.hash), key.index);
          }
          if self.max_slot - slot > 10000 {
              error!("Future spend UTXO {}:{} from slot {slot} is too old (max slot {})",
                     encode(key.hash), key.index, self.max_slot);
          }
      }
    }

}

