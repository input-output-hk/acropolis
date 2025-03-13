//! Acropolis UTXOState: State storage
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use acropolis_common::SerialisedMessageHandler;
use acropolis_messages::{BlockInfo, BlockStatus, TxInput, TxOutput, UTXODelta, UTXODeltasMessage};
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
    pub fn observe_block(&mut self, block: &BlockInfo) {

        match block.status {
            BlockStatus::RolledBack => {
                info!(slot = block.slot, number = block.number, "Rollback received");

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

            _ => {}
        }

        self.last_slot = block.slot;
        self.last_number = block.number;
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
    fn prune(&mut self) {
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
    fn log_stats(&self) {
        info!(slot = self.last_slot,
            number = self.last_number,
            total_utxos = self.utxos.len(),
            valid_utxos = self.count_valid_utxos());
    }

    /// Tick for pruning and logging
    pub fn tick(&mut self) {
        self.prune();
        self.log_stats();
    }
}

impl SerialisedMessageHandler<UTXODeltasMessage> for State {

    /// Handle a message
    fn handle(&mut self, deltas: &UTXODeltasMessage) {

       // Observe block for stats and rollbacks
       self.observe_block(&deltas.block);

       // Process the deltas
       for delta in &deltas.deltas {  // UTXODelta

           match delta {
               UTXODelta::Input(tx_input) => {
                   self.observe_input(&tx_input, deltas.block.number);
               }, 

               UTXODelta::Output(tx_output) => {
                   self.observe_output(&tx_output, deltas.block.number);
               },

               _ => {}
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
        assert_eq!(0, state.last_slot);
        assert_eq!(0, state.last_number);
        assert_eq!(0, state.count_valid_utxos());
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
    fn observe_input_spends_utxo() {
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
    fn rollback_removes_future_created_utxos() {
        let mut state = State::new();
        let output = TxOutput {
           tx_hash: vec!(42),
           index: 0,
           address: vec!(99),
           value: 42,
        };

        state.observe_output(&output, 10);
        assert_eq!(1, state.utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        let block = BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 200,
            number: 9,
            hash: vec!(),
        };

        state.observe_block(&block);

        assert_eq!(0, state.utxos.len());
        assert_eq!(0, state.count_valid_utxos());
    }

    #[test]
    fn rollback_reinstates_future_spent_utxos() {
        let mut state = State::new();

        // Create the UTXO in block 10
        let output = TxOutput {
           tx_hash: vec!(42),
           index: 0,
           address: vec!(99),
           value: 42,
        };

        state.observe_output(&output, 10);
        assert_eq!(1, state.utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        // Spend it in block 15
        let input = TxInput {
            tx_hash: output.tx_hash,
            index: output.index,
        };

        state.observe_input(&input, 15);
        assert_eq!(1, state.utxos.len());
        assert_eq!(0, state.count_valid_utxos());

        // Roll back to 12
        let block = BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 200,
            number: 12,
            hash: vec!(),
        };

        state.observe_block(&block);

        // Should be reinstated
        assert_eq!(1, state.utxos.len());
        assert_eq!(1, state.count_valid_utxos());
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

        state.observe_block(&block);
        assert_eq!(5483, state.last_number);

        state.prune();
        assert_eq!(0, state.utxos.len());
    }

}