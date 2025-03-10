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
    address: Vec<u8>, //< Address
    value: u64,       //< Value in Lovelace

    // Lifetime - note that a UTXO can be spent but not created if they arrive out of order
    created_at: Option<u64>, //< Block number UTXO was created (output), if any
    spent_at: Option<u64>,   //< Block number UTXO was spent (input), if any
}

/// Ledger state storage
pub struct State {
    utxos: HashMap<UTXOKey, UTXOValue>,    //< Live UTXOs
    max_slot: u64,                         //< Maximum block slot received
    max_number: u64,                       //< Maximum block number received
}

impl State {
    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            utxos: HashMap::new(),
            max_slot: 0,
            max_number: 0,
        }
    }

    /// Look up a UTXO
    #[cfg(test)] // until used outside
    pub fn lookup_utxo(&self, key: &UTXOKey) -> Option<&UTXOValue> {
        return self.utxos.get(key);
    }

    /// Get the number of valid UTXOs - that is, that have a valid created_at
    /// but no spent_at
    pub fn count_valid_utxos(&self) -> usize {
        return self.utxos.values().filter(
            |value| value.spent_at.is_none() && value.created_at.is_some()
        ).count(); 
    }

    /// Get the number of future (out of order) UTXOs - that is, that have a
    /// spent_at but no created_at
    pub fn count_future_utxos(&self) -> usize {
        return self.utxos.values().filter(
            |value| value.spent_at.is_some() && value.created_at.is_none()
        ).count(); 
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
    pub fn observe_input(&mut self, input: &TxInput, block_number: u64) {
        let key = UTXOKey::new(&input.tx_hash, input.index);
        
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO << {}:{}", encode(&key.hash), key.index);
        }

        // UTXO exists?
        match self.utxos.get_mut(&key) {
            Some(utxo) => {
                // Normal case - just mark as spent in this block
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("        - spent {} from {}",
                           utxo.value, encode(utxo.address.clone()));
                }

                utxo.spent_at = Some(block_number);
            }
            _ => {
                // Out-of-order case - since we assume spend of a non-existent
                // UTXO can never happen in a valid chain, it must have arrived
                // out of order - we mark it as spent but not created yet
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("UTXO {}:{} arrived out of order (block {})",
                           encode(&key.hash), key.index, block_number);
                }

                // Create already spent UTXO, with no created_at
                self.utxos.insert(key, UTXOValue {
                    address: Vec::new(),  // Not known yet
                    value: 0,
                    created_at: None,
                    spent_at: Some(block_number),
                });
            }
        }
    } 

    /// Observe an output UXTO creation
    pub fn observe_output(&mut self,  output: &TxOutput, block_number: u64) {

        let key = UTXOKey::new(&output.tx_hash, output.index);

        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO >> {}:{}", encode(&key.hash), key.index);
            debug!("        - adding {} to {}", output.value, encode(&output.address));
        }

        // Check if it was spent in a future that block arrived out of order
        match self.utxos.get_mut(&key) {
            Some(utxo) => {

                // Already seen - unless created twice (impossible) then it must be one that
                // arrived out of order
                match utxo.spent_at {
                    Some(spent_block_number) => {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!("UTXO {}:{} already seen (created in block {}, spent in block {})",
                                encode(&key.hash), key.index, block_number, spent_block_number);
                        }

                        // We just mark it as created now, with the value - although note that
                        // it's already spent, so this value doesn't accumulate to anything
                        utxo.created_at = Some(block_number);
                        utxo.address = output.address.clone();
                        utxo.value = output.value;
                    }

                    _ => error!("Saw UTXO {}:{} before but not spent!",
                        encode(&key.hash), key.index)
                }
            }

            _ => {
                // Normal case - insert a new UTXO, created but not spent
                self.utxos.insert(key, UTXOValue {
                    address: output.address.clone(),
                    value: output.value,
                    created_at: Some(block_number),
                    spent_at: None,
                });
            }
        }
    }

    /// Log statistics
    pub fn log_stats(&self) {
        info!(slot = self.max_slot,
            number = self.max_number,
            total_utxos = self.utxos.len(),
            valid_utxos = self.count_valid_utxos(),
            future_utxos = self.count_future_utxos());
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
        assert_eq!(0, state.max_slot);
        assert_eq!(0, state.max_number);
        assert_eq!(0, state.count_valid_utxos());
        assert_eq!(0, state.count_future_utxos());
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
        assert_eq!(1, state.count_valid_utxos());

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
        assert_eq!(1, state.count_valid_utxos());

        let input = TxInput {
            tx_hash: output.tx_hash,
            index: output.index,
        };

        state.observe_input(&input, 2);
        assert_eq!(1, state.utxos.len());
        assert_eq!(0, state.count_valid_utxos());
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

        assert_eq!(1, state.utxos.len());
        assert_eq!(0, state.count_valid_utxos());

        let output = TxOutput {
           tx_hash: vec!(42),
           index: 0,
           address: vec!(99),
           value: 42,
        };

        state.observe_output(&output, 1);
        assert_eq!(1, state.utxos.len());
        assert_eq!(0, state.count_valid_utxos());
    }

}