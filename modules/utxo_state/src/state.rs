//! Acropolis UTXOState: State storage
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use acropolis_common::{
    SerialisedMessageHandler,
    Address, BlockInfo, BlockStatus, 
    TxInput, TxOutput, UTXODelta,
    messages::UTXODeltasMessage
};
use tracing::{debug, info, error};
use hex::encode;
use std::sync::Arc;
use async_trait::async_trait;

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
    pub address: Address,

    /// Value in Lovelace
    pub value: u64,
}

/// Address delta observer
/// Note all methods are immutable to avoid locking in state - use channels if
/// required
#[async_trait]
pub trait AddressDeltaObserver: Send + Sync + 'static {
    /// Observe a new block
    async fn start_block(&self, block: &BlockInfo);

    /// Observe a delta
    async fn observe_delta(&self, address: &Address, delta: i64);

    /// Finalise a block, with the given event sequence
    async fn finalise_block(&self, block: &BlockInfo, sequence: u64);
}

/// Ledger state storage
pub struct State {
    /// Last slot number received
    last_slot: u64,

    /// Last block number received
    last_number: u64,

    /// Volatile UTXOs
    volatile_utxos: HashMap<UTXOKey, UTXOValue>,

    /// Index of volatile UTXOs by created block
    volatile_created: VolatileIndex,

    /// Index of volatile UTXOs by spent block
    volatile_spent: VolatileIndex,

    /// Address delta observer
    address_delta_observer: Option<Arc<dyn AddressDeltaObserver>>,

    /// Immutable UTXOs
    // TODO move to an ImmutableStorageProvider
    immutable_utxos: HashMap<UTXOKey, UTXOValue>,
}

impl State {
    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            last_slot: 0,
            last_number: 0,
            volatile_utxos: HashMap::new(),
            volatile_created: VolatileIndex::new(),
            volatile_spent: VolatileIndex::new(),
            address_delta_observer: None,

            immutable_utxos: HashMap::new(),
        }
    }

    /// Register the delta observer
    pub fn register_address_delta_observer(&mut self, 
            observer: Arc<dyn AddressDeltaObserver>) {
        self.address_delta_observer = Some(observer);
    }

    /// Look up a UTXO
    pub fn lookup_utxo(&self, key: &UTXOKey) -> Option<&UTXOValue> {
        match self.volatile_utxos.get(key) {
            Some(key) => Some(key),
            None => self.immutable_utxos.get(key)
        }
    }

    /// Get the number of valid UTXOs - that is, that have a valid created_at
    /// but no spent_at
    pub fn count_valid_utxos(&self) -> usize {
        return self.volatile_utxos.len() - self.volatile_spent.len()
             + self.immutable_utxos.len()
    }

    /// Observe a block for statistics and handle rollbacks
    pub async fn observe_block(&mut self, block: &BlockInfo) {

        match block.status {
            BlockStatus::RolledBack => {
                info!(slot = block.slot, number = block.number, "Rollback received");

                // Delete all UTXOs created in or after this block
                let utxos = self.volatile_created.prune_on_or_after(block.number);
                for key in utxos {
                    if let Some(utxo) = self.volatile_utxos.remove(&key) {
                        // Tell the observer to debit it
                        if let Some(observer) = self.address_delta_observer.as_ref() {
                            observer.observe_delta(&utxo.address, -(utxo.value as i64)).await;
                        }                                 
                    }
                };

                // Any remaining (which were necessarily created before this block)
                // that were spent in or after this block can be reinstated
                let utxos = self.volatile_spent.prune_on_or_after(block.number);
                for key in utxos {
                    if let Some(utxo) = self.volatile_utxos.get(&key) {
                        // Tell the observer to recredit it
                        if let Some(observer) = self.address_delta_observer.as_ref() {
                            observer.observe_delta(&utxo.address, utxo.value as i64).await;
                        } 
                    }
                };

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
    pub async fn observe_input(&mut self, input: &TxInput, block: &BlockInfo) {
        let key = UTXOKey::new(&input.tx_hash, input.index);
        
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO << {}:{}", encode(&key.hash), key.index);
        }

        // UTXO exists?
        match self.lookup_utxo(&key) {
            Some(utxo) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("        - spent {} from {:?}", utxo.value, utxo.address);
                }

                // Tell the observer it's spent
                if let Some(observer) = self.address_delta_observer.as_ref() {
                    observer.observe_delta(&utxo.address, -(utxo.value as i64)).await;
                }        

                match block.status {
                    BlockStatus::Volatile | BlockStatus::RolledBack => {
                        // Add to volatile spent index
                        self.volatile_spent.add_utxo(&key);
                    }
                    BlockStatus::Bootstrap | BlockStatus::Immutable => {
                        // Immutable - we can delete it immediately
                        self.immutable_utxos.remove(&key);
                    }
                }
            }
            _ => {
                error!("UTXO {}:{} unknown in block {}",
                    encode(&key.hash), key.index, block.number);
            }
        }
    } 

    /// Observe an output UXTO creation
    pub async fn observe_output(&mut self,  output: &TxOutput, block: &BlockInfo) {

        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO >> {}:{}", encode(&output.tx_hash), output.index);
            debug!("        - adding {} to {:?}", output.value, output.address);
        }

        // Insert the UTXO, checking if it already existed
        let key = UTXOKey::new(&output.tx_hash, output.index);

        let value = UTXOValue {
            address: output.address.clone(),
            value: output.value,
        };

        // Add to volatile or immutable maps
        let previous = match block.status {
            BlockStatus::Volatile | BlockStatus::RolledBack => {
                self.volatile_created.add_utxo(&key);
                self.volatile_utxos.insert(key, value)
            }
            BlockStatus::Bootstrap | BlockStatus::Immutable => {
                self.immutable_utxos.insert(key, value)
            }
        };

        // Add to full UTXO map
        if previous.is_some() {
            error!("Saw UTXO {}:{} before, in block {}",
                encode(&output.tx_hash), output.index, block.number);
        }

        // Tell the observer
        if let Some(observer) = self.address_delta_observer.as_ref() {
            observer.observe_delta(&output.address, output.value as i64).await;
        }        
    }

    /// Background prune
    async fn prune(&mut self) {
        // Remove all volatile UTXOs that have now become immutably spent
        // and transfer unspent ones to immutable
        if self.last_number >= SECURITY_PARAMETER_K {
            let boundary = self.last_number - SECURITY_PARAMETER_K;

            // Find all UTXOs in the volatile index spent before this boundary
            // and remove from both maps
            let spent_utxos = self.volatile_spent.prune_before(boundary);
            if !spent_utxos.is_empty() {
                info!("Removing {} immutably spent UTXOs", spent_utxos.len());
                for key in spent_utxos { 
                    // Remove from volatile, and only if not there, from immutable
                    if self.volatile_utxos.remove(&key).is_none() {
                        self.immutable_utxos.remove(&key);
                    }
                }
            }   

            // Prune the created index too, and transfer the UTXOs to immutable
            let created_utxos = self.volatile_created.prune_before(boundary);
            if !created_utxos.is_empty() {
                info!("Moving {} volatile UTXOs into immutable", created_utxos.len());
                for key in created_utxos {
                    let value = self.volatile_utxos.remove(&key);
                    if let Some(value) = value {
                        self.immutable_utxos.insert(key, value);
                    }
                }
            }

            self.volatile_utxos.shrink_to_fit();
        }
    }

    /// Log statistics
    fn log_stats(&self) {
        info!(slot = self.last_slot,
            number = self.last_number,
            immutable_utxos = self.immutable_utxos.len(),
            volatile_utxos = self.volatile_utxos.len(),
            valid_utxos = self.count_valid_utxos());
    }

    /// Tick for pruning and logging
    pub async fn tick(&mut self) {
        self.prune().await;
        self.log_stats();
    }
}

#[async_trait]
impl SerialisedMessageHandler<UTXODeltasMessage> for State {

    /// Handle a message
    async fn handle(&mut self, deltas: &UTXODeltasMessage) {

        // Start the block for observer
        if let Some(observer) = self.address_delta_observer.as_mut() {
            observer.start_block(&deltas.block).await;
        }

        // Observe block for stats and rollbacks
        self.observe_block(&deltas.block).await;

        // Process the deltas
        for delta in &deltas.deltas {  // UTXODelta

           match delta {
               UTXODelta::Input(tx_input) => {
                   self.observe_input(&tx_input, &deltas.block).await;
               }, 

               UTXODelta::Output(tx_output) => {
                   self.observe_output(&tx_output, &deltas.block).await;
               },

               _ => {}
           }
       }

        // End the block for observer
        if let Some(observer) = self.address_delta_observer.as_mut() {
            observer.finalise_block(&deltas.block, deltas.sequence).await;
        }
    
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::ByronAddress;
    use tokio::sync::Mutex;

    // Create an address for testing - we use Byron just because it's easier to
    // create and test the payload
    fn create_address(n: u8) -> Address {
        Address::Byron(ByronAddress {
            payload: vec!(n)
        })
    }

    #[test]
    fn new_state_is_empty() {
        let state = State::new();
        assert_eq!(0, state.last_slot);
        assert_eq!(0, state.last_number);
        assert_eq!(0, state.count_valid_utxos());
        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(0, state.immutable_utxos.len());
    }

    #[tokio::test]
    async fn observe_output_adds_to_immutable_utxos() {
        let mut state = State::new();
        let output = TxOutput {
           tx_hash: vec!(42),
           index: 0,
           address: create_address(99),
           value: 42,
        };

        let block = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 1,
            number: 1,
            hash: vec!(),
        };

        state.observe_output(&output, &block).await;
        assert_eq!(1, state.immutable_utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        let key = UTXOKey::new(&output.tx_hash, output.index);
        match state.lookup_utxo(&key) {
            Some(value) => {
                assert!(matches!(&value.address, Address::Byron(ByronAddress{ payload }) 
                    if payload[0] == 99));
                assert_eq!(42, value.value);
            },

            _ => panic!("UTXO not found")
        }
    }

    #[tokio::test]
    async fn observe_input_spends_utxo() {
        let mut state = State::new();
        let output = TxOutput {
            tx_hash: vec!(42),
            index: 0,
            address: create_address(99),
            value: 42,
        };

        let block1 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 1,
            number: 1,
            hash: vec!(),
        };

        state.observe_output(&output, &block1).await;
        assert_eq!(1, state.immutable_utxos.len());
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

        state.observe_input(&input, &block2).await;
        assert_eq!(0, state.immutable_utxos.len());
        assert_eq!(0, state.count_valid_utxos());
    }

    #[tokio::test]
    async fn rollback_removes_future_created_utxos() {
        let mut state = State::new();
        let output = TxOutput {
            tx_hash: vec!(42),
            index: 0,
            address: create_address(99),
            value: 42,
        };

        let block10 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 10,
            number: 10,
            hash: vec!(),
        };

        state.observe_block(&block10).await;
        state.observe_output(&output, &block10).await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        let block9 = BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 200,
            number: 9,
            hash: vec!(),
        };

        state.observe_block(&block9).await;

        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos());
    }

    #[tokio::test]
    async fn rollback_reinstates_future_spent_utxos() {
        let mut state = State::new();

        // Create the UTXO in block 10
        let output = TxOutput {
            tx_hash: vec!(42),
            index: 0,
            address: create_address(99),
            value: 42,
        };

        let block10 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 10,
            number: 10,
            hash: vec!(),
        };

        state.observe_block(&block10).await;
        state.observe_output(&output, &block10).await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        // Spend it in block 11
        let input = TxInput {
            tx_hash: output.tx_hash,
            index: output.index,
        };

        let block11 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 11,
            number: 11,
            hash: vec!(),
        };

        state.observe_block(&block11).await;
        state.observe_input(&input, &block11).await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos());

        // Roll back to 11
        let block11_2= BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 200,
            number: 11,
            hash: vec!(),
        };

        state.observe_block(&block11_2).await;

        // Should be reinstated
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos());
    }

    #[tokio::test]
    async fn prune_shifts_new_utxos_into_immutable() {
        let mut state = State::new();
        let output = TxOutput {
            tx_hash: vec!(42),
            index: 0,
            address: create_address(99),
            value: 42,
        };

        let block1 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 1,
            number: 1,
            hash: vec!(),
        };

        state.observe_block(&block1).await;
        state.observe_output(&output, &block1).await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.immutable_utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        // Prune shouldn't do anything yet
        state.prune().await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.immutable_utxos.len());

        // Observe a block much later
        let block = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 23492,
            number: 5483,
            hash: vec!(),
        };

        state.observe_block(&block).await;
        assert_eq!(5483, state.last_number);

        state.prune().await;
        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(1, state.immutable_utxos.len());
    }

    #[tokio::test]
    async fn prune_deletes_old_spent_utxos() {
        let mut state = State::new();
        let output = TxOutput {
            tx_hash: vec!(42),
            index: 0,
            address: create_address(99),
            value: 42,
        };

        let block1 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 1,
            number: 1,
            hash: vec!(),
        };

        state.observe_block(&block1).await;
        state.observe_output(&output, &block1).await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos());

        let input = TxInput {
            tx_hash: output.tx_hash,
            index: output.index,
        };

        let block2 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 2,
            number: 2,
            hash: vec!(),
        };

        state.observe_block(&block2).await;
        state.observe_input(&input, &block2).await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos());

        // Prune shouldn't do anything yet
        state.prune().await;
        assert_eq!(1, state.volatile_utxos.len());

        // Observe a block much later
        let block = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 23492,
            number: 5483,
            hash: vec!(),
        };

        state.observe_block(&block).await;
        assert_eq!(5483, state.last_number);

        state.prune().await;
        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(0, state.immutable_utxos.len());
    }

    struct TestDeltaObserver {
        balance: Mutex<i64>,
    }

    impl TestDeltaObserver {
        fn new() -> Self {
            Self { balance: Mutex::new(0) }
        }
    }

    #[async_trait]
    impl AddressDeltaObserver for TestDeltaObserver {
        async fn start_block(&self, _block: &BlockInfo) {
            
        }
        async fn observe_delta(&self, address: &Address, delta: i64) {
            assert!(matches!(&address, Address::Byron(ByronAddress{ payload }) 
                if payload[0] == 99));
            assert!(delta == 42 || delta == -42);

            let mut balance = self.balance.lock().await;
            *balance += delta;
        }
        async fn finalise_block(&self, _block: &BlockInfo, _next_sequence: u64) {
            
        }
    }

    #[tokio::test]
    async fn observe_output_then_input_notifies_net_0_balance_change() {
        let mut state = State::new();
        let observer = Arc::new(TestDeltaObserver::new());
        state.register_address_delta_observer(observer.clone());

        let output = TxOutput {
            tx_hash: vec!(42),
            index: 0,
            address: create_address(99),
            value: 42,
        };

        let block1 = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 1,
            number: 1,
            hash: vec!(),
        };

        state.observe_output(&output, &block1).await;
        assert_eq!(1, state.immutable_utxos.len());
        assert_eq!(1, state.count_valid_utxos());
        assert_eq!(42, *observer.balance.lock().await);

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

        state.observe_input(&input, &block2).await;
        assert_eq!(0, state.immutable_utxos.len());
        assert_eq!(0, state.count_valid_utxos());
        assert_eq!(0, *observer.balance.lock().await);
    }

    #[tokio::test]
    async fn observe_rollback_notifies_balance_debit_on_future_created_utxos() {
        let mut state = State::new();
        let observer = Arc::new(TestDeltaObserver::new());
        state.register_address_delta_observer(observer.clone());

        let output = TxOutput {
            tx_hash: vec!(42),
            index: 0,
            address: create_address(99),
            value: 42,
        };

        let block10 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 10,
            number: 10,
            hash: vec!(),
        };

        state.observe_block(&block10).await;
        state.observe_output(&output, &block10).await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos());
        assert_eq!(42, *observer.balance.lock().await);

        let block9 = BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 200,
            number: 9,
            hash: vec!(),
        };

        state.observe_block(&block9).await;

        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos());
        assert_eq!(0, *observer.balance.lock().await);
    }

    #[tokio::test]
    async fn observe_rollback_notifies_balance_credit_on_future_spent_utxos() {
        let mut state = State::new();
        let observer = Arc::new(TestDeltaObserver::new());
        state.register_address_delta_observer(observer.clone());

        // Create the UTXO in block 10
        let output = TxOutput {
            tx_hash: vec!(42),
            index: 0,
            address: create_address(99),
            value: 42,
        };

        let block10 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 10,
            number: 10,
            hash: vec!(),
        };

        state.observe_block(&block10).await;
        state.observe_output(&output, &block10).await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos());
        assert_eq!(42, *observer.balance.lock().await);

        // Spend it in block 11
        let input = TxInput {
            tx_hash: output.tx_hash,
            index: output.index,
        };

        let block11 = BlockInfo {
            status: BlockStatus::Volatile,
            slot: 11,
            number: 11,
            hash: vec!(),
        };

        state.observe_block(&block11).await;
        state.observe_input(&input, &block11).await;
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos());
        assert_eq!(0, *observer.balance.lock().await);

        // Roll back to 11
        let block11_2= BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 200,
            number: 11,
            hash: vec!(),
        };

        state.observe_block(&block11_2).await;

        // Should be reinstated
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos());
        assert_eq!(42, *observer.balance.lock().await);
    }

}