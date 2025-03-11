//! Acropolis UTXOState: State storage
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use acropolis_messages::{BlockInfo, BlockStatus, TxInput, TxOutput};
use tracing::{debug, info, error};
use hex::encode;

const SECURITY_PARAMETER_K: u64 = 2160;

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
    /// Address in binary
    address: Vec<u8>,

    /// Value in Lovelace
    value: u64,

    /// Block number UTXO was created (output)
    created_at: u64,

    /// Block number UTXO was spent (input), if any
    spent_at: Option<u64>,   
}

/// Ledger state storage
pub struct State {
    /// Live UTXOs
    utxos: HashMap<UTXOKey, UTXOValue>,

    /// Last slot number received
    last_slot: u64,

    /// Last block number received
    last_number: u64,
}

impl State {
    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            utxos: HashMap::new(),
            last_slot: 0,
            last_number: 0,
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
        return self.utxos.values().filter(|value| value.spent_at.is_none())
            .count(); 
    }

    /// Observe a block for statistics and handle rollbacks
    /// Returns whether the block was accepted (number is 1 more than max_number)
    pub fn observe_block(&mut self, block: &BlockInfo) -> bool {

        // Double check we don't see rewinds or duplicates
        if block.number <= self.last_number && block.number != 0 {
            error!("Block {} received expecting {} - ignored!",
                block.number, self.last_number+1);
            return true;  // Pretend we processed it
        }

        // They must arrive in order at this point - but note we receive two block 0's,
        // one from genesis, one from the chain
        if block.number != self.last_number + 1 && block.number != 0 {
            return false;
        }

        // Note we use max because the genesis block can arrive at any point
        self.last_slot = self.last_slot.max(block.slot);
        self.last_number = self.last_number.max(block.number);

        if matches!(block.status, BlockStatus::RolledBack) {
            info!(slot = block.slot, number = block.number,
                "Rollback received");

            // Check all UTXOs - any created in or after this can be deleted
            self.utxos.retain(|_, value| value.created_at < block.number);
           
            // Any remaining (which were necessarily created before this block)
            // that were spent in or after this block can be reinstated
            for value in self.utxos.values_mut() {
                match value.spent_at {
                    Some(number) if number >= block.number => value.spent_at = None,
                    _ => {} 
                }
           }

           // Let the pruner compress the map
        }

        return true;

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
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("        - spent {} from {}",
                           utxo.value, encode(utxo.address.clone()));
                }

                // Just mark as spent in this block
                utxo.spent_at = Some(block_number);
            }
            _ => {
                error!("UTXO {}:{} unknown in block {}",
                    encode(&key.hash), key.index, block_number);
            }
        }
    } 

    /// Observe an output UXTO creation
    pub fn observe_output(&mut self,  output: &TxOutput, block_number: u64) {

        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO >> {}:{}", encode(&output.tx_hash), output.index);
            debug!("        - adding {} to {}", output.value, encode(&output.address));
        }

        // Insert the UTXO, checking if it already existed
        let key = UTXOKey::new(&output.tx_hash, output.index);
        if let Some(_) = self.utxos.insert(key, UTXOValue {
            address: output.address.clone(),
            value: output.value,
            created_at: block_number,
            spent_at: None,
        }) {
            error!("Saw UTXO {}:{} before, in block {block_number}",
                encode(&output.tx_hash), output.index);
        }
    }

    /// Background prune
    pub fn prune(&mut self) {
        // Remove all UTXOs which were spent older than 'k' before max_number
        if self.last_number >= SECURITY_PARAMETER_K {
            let boundary = self.last_number - SECURITY_PARAMETER_K;
            self.utxos.retain(|_, value| match value.spent_at {
                Some(number) => number >= boundary,
                _ => true
            });
            self.utxos.shrink_to_fit();
        }
    }

    /// Log statistics
    pub fn log_stats(&self) {
        info!(slot = self.last_slot,
            number = self.last_number,
            total_utxos = self.utxos.len(),
            valid_utxos = self.count_valid_utxos());
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
        assert_eq!(0, state.last_slot);
        assert_eq!(0, state.last_number);
        assert_eq!(0, state.count_valid_utxos());
    }

    #[test]
    fn observe_block_accepts_only_serial_input() {
        let mut state = State::new();
        let block1 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 99,
            number: 1,
            hash: vec!(),
        };

        assert!(state.observe_block(&block1));
        assert_eq!(99, state.last_slot);
        assert_eq!(1, state.last_number);

        let block2 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 200,  // Can't happen but tests max
            number: 3,  // Out of order
            hash: vec!(),
        };

        assert!(!state.observe_block(&block2));
        assert_eq!(99, state.last_slot);
        assert_eq!(1, state.last_number);
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
    fn prune_deletes_old_spent_utxos() {
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

        // Prune shouldn't do anything yet
        state.prune();
        assert_eq!(1, state.utxos.len());

        // Observe a block much later
        let block = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 23492,
            number: 5483,
            hash: vec!(),
        };

        // Fudge so it accepts the block
        state.last_number = 5482;
        assert!(state.observe_block(&block));
        assert_eq!(5483, state.last_number);

        state.prune();
        assert_eq!(0, state.utxos.len());
    }

}