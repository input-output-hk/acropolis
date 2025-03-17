//! Acropolis UTXOState: State storage
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use acropolis_common::SerialisedMessageHandler;
use acropolis_messages::{
    BlockInfo, BlockStatus, TxInput, TxOutput,
    UTXODelta, UTXODeltasMessage
};
use tracing::{debug, info, error};
use hex::encode;

use crate::volatile_index::VolatileIndex;

const SECURITY_PARAMETER_K: u64 = 2160;

/// Key of ledger state store
#[derive(Debug, Clone, Eq)]
pub struct UTXOKey {
    pub hash: [u8; 32], // Tx hash
    pub index: u64,     // Output index in the transaction
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
    pub address: Vec<u8>,

    /// Value in Lovelace
    pub value: u64,

    /// Block number UTXO was spent (input), if any
    pub spent_at: Option<u64>,   
}

/// Ledger state storage
pub struct State {
    /// Live UTXOs
    utxos: HashMap<UTXOKey, UTXOValue>,

    /// Last slot number received
    last_slot: u64,

    /// Last block number received
    last_number: u64,

    /// Index of volatile UTXOs by created block
    volatile_created: VolatileIndex,

    /// Index of volatile UTXOs by spent block
    volatile_spent: VolatileIndex,
}

impl State {
    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            utxos: HashMap::new(),
            last_slot: 0,
            last_number: 0,
            volatile_created: VolatileIndex::new(),
            volatile_spent: VolatileIndex::new(),
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

                // Delete all UTXOs created in or after this block
                self.volatile_created.prune_on_or_after(block.number, |key| { 
                    self.utxos.remove(key);
                });

                // Any remaining (which were necessarily created before this block)
                // that were spent in or after this block can be reinstated
                self.volatile_spent.prune_on_or_after(block.number, |key| {
                    if let Some(utxo) = self.utxos.get_mut(&key) {
                        // Just mark as unspent
                        utxo.spent_at = None;
                    }
                });

                // Let the pruner compress the map
            }

            _ => {}
        }

        self.last_slot = block.slot;
        self.last_number = block.number;

        // Add to index only if volatile or rolled-back volatile
        // Note avoids issues with duplicate block 0, which aren't
        match block.status {
            BlockStatus::Volatile | BlockStatus::RolledBack => {
                self.volatile_created.add_block(block.number);
                self.volatile_spent.add_block(block.number);
            }

            _ => {}
        }
    }

    /// Observe an input UTXO spend
    pub fn observe_input(&mut self, input: &TxInput, block: &BlockInfo) {
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
                utxo.spent_at = Some(block.number);

                // Add to volatile spent index
                match block.status {
                    BlockStatus::Volatile | BlockStatus::RolledBack => {
                        self.volatile_spent.add_utxo(&key);
                    }
                    _ => {}
                }
            }
            _ => {
                error!("UTXO {}:{} unknown in block {}",
                    encode(&key.hash), key.index, block.number);
            }
        }
    } 

    /// Observe an output UXTO creation
    pub fn observe_output(&mut self,  output: &TxOutput, block: &BlockInfo) {

        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO >> {}:{}", encode(&output.tx_hash), output.index);
            debug!("        - adding {} to {}", output.value, encode(&output.address));
        }

        // Insert the UTXO, checking if it already existed
        let key = UTXOKey::new(&output.tx_hash, output.index);

        // Add to volatile index
        match block.status {
            BlockStatus::Volatile | BlockStatus::RolledBack => {
                self.volatile_created.add_utxo(&key);
            }
            _ => {}
        }

        // Add to full UTXO map
        if self.utxos.insert(key, UTXOValue {
            address: output.address.clone(),
            value: output.value,
            spent_at: None,
        }).is_none() {
        } else {
            error!("Saw UTXO {}:{} before, in block {}",
                encode(&output.tx_hash), output.index, block.number);
        }

    }

    /// Background prune
    fn prune(&mut self) {
        // Remove all UTXOs that have now become immutably spent
        if self.last_number >= SECURITY_PARAMETER_K {
            let boundary = self.last_number - SECURITY_PARAMETER_K;

            // Find all UTXOs in the volatile index spent before this boundary
            // and remove from main map
            self.volatile_spent.prune_before(boundary, |key| {
                self.utxos.remove(key);
            });

            // Prune the created index too, but leave the UTXOs
            self.volatile_created.prune_before(boundary, |_| {});

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
                   self.observe_input(&tx_input, &deltas.block);
               }, 

               UTXODelta::Output(tx_output) => {
                   self.observe_output(&tx_output, &deltas.block);
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

        let block = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 1,
            number: 1,
            hash: vec!(),
        };

        state.observe_output(&output, &block);
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

        let block1 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 1,
            number: 1,
            hash: vec!(),
        };

        state.observe_output(&output, &block1);
        assert_eq!(1, state.utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        let input = TxInput {
            tx_hash: output.tx_hash,
            index: output.index,
        };


        let block2 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 2,
            number: 2,
            hash: vec!(),
        };

        state.observe_input(&input, &block2);
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

        let block10 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 10,
            number: 10,
            hash: vec!(),
        };

        state.observe_output(&output, &block10);
        assert_eq!(1, state.utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        let block9 = BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 200,
            number: 9,
            hash: vec!(),
        };

        state.observe_block(&block9);

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

        let block10 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 10,
            number: 10,
            hash: vec!(),
        };

        state.observe_output(&output, &block10);
        assert_eq!(1, state.utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        // Spend it in block 15
        let input = TxInput {
            tx_hash: output.tx_hash,
            index: output.index,
        };

        let block15 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 15,
            number: 15,
            hash: vec!(),
        };

        state.observe_input(&input, &block15);
        assert_eq!(1, state.utxos.len());
        assert_eq!(0, state.count_valid_utxos());

        // Roll back to 12
        let block12= BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 200,
            number: 12,
            hash: vec!(),
        };

        state.observe_block(&block12);

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

        let block1 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 1,
            number: 1,
            hash: vec!(),
        };

        state.observe_output(&output, &block1);
        assert_eq!(1, state.utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        let input = TxInput {
            tx_hash: output.tx_hash,
            index: output.index,
        };

        let block2 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 2,
            number: 2,
            hash: vec!(),
        };

        state.observe_input(&input, &block2);
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