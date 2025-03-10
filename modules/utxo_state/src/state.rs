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

    /// Look up a UTXO
    #[cfg(test)] // until used outside
    pub fn lookup_utxo(&self, key: &UTXOKey) -> Option<&UTXOValue> {
        return self.utxos.get(key);
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

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_is_empty() {
        let state = State::new();
        assert_eq!(0, state.utxos.len());
        assert_eq!(0, state.future_spends.len());
        assert_eq!(0, state.max_slot);
        assert_eq!(0, state.max_number);
    }

    #[test]
    fn observe_block_gathers_maxima() {
        let mut state = State::new();
        let block1 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 99,
            number: 42,
            hash: vec!(),
        };

        state.observe_block(&block1);
        assert_eq!(99, state.max_slot);
        assert_eq!(42, state.max_number);

        let block2 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 98,  // Can't happen but tests max
            number: 43,
            hash: vec!(),
        };

        state.observe_block(&block2);
        assert_eq!(99, state.max_slot);
        assert_eq!(43, state.max_number);
    }

    #[test]
    fn observe_output_adds_to_utxos() {
        let mut state = State::new();
        let output = TxOutput {
           tx_hash: vec!(42),
           index: 0,
           address: vec!(99),
           value: 42,
        };

        state.observe_output(&output, 1);
        assert_eq!(1, state.utxos.len());

        let key = UTXOKey::new(&output.tx_hash, output.index);
        match state.lookup_utxo(&key) {
            Some(value) => {
                assert_eq!(99, *value.address.get(0).unwrap());
                assert_eq!(42, value.value);
            },

            _ => panic!("UTXO not found")
        }
    }

    #[test]
    fn observe_output_then_input_spends_utxo() {
        let mut state = State::new();
        let output = TxOutput {
           tx_hash: vec!(42),
           index: 0,
           address: vec!(99),
           value: 42,
        };

        state.observe_output(&output, 1);
        assert_eq!(1, state.utxos.len());

        let input = TxInput {
            tx_hash: output.tx_hash,
            index: output.index,
        };

        state.observe_input(&input, 2);
        assert_eq!(0, state.utxos.len());
    }

    #[test]
    fn observe_input_then_output_spends_utxo() {
        let mut state = State::new();

        // Input received first
        let input = TxInput {
            tx_hash: vec!(42),
            index: 0,
        };
        
        state.observe_input(&input, 2);

        assert_eq!(0, state.utxos.len());
        assert_eq!(1, state.future_spends.len());

        let output = TxOutput {
           tx_hash: vec!(42),
           index: 0,
           address: vec!(99),
           value: 42,
        };

        state.observe_output(&output, 1);
        assert_eq!(0, state.utxos.len());
        assert_eq!(0, state.future_spends.len());
    }

}