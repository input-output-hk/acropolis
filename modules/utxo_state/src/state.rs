//! Acropolis UTXOState: State storage
use crate::volatile_index::VolatileIndex;
use acropolis_common::{
    messages::UTXODeltasMessage, params::SECURITY_PARAMETER_K, Address, BlockInfo, BlockStatus,
    Datum, TxInput, TxOutput, UTXODelta,
};
use acropolis_common::{UTxOIdentifier, Value, ValueDelta};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info};

/// Value stored in UTXO
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXOValue {
    /// Address in binary
    pub address: Address,

    /// Value in Lovelace
    pub value: Value,

    /// Datum
    pub datum: Option<Datum>,
}

/// Address delta observer
/// Note all methods are immutable to avoid locking in state - use channels
/// or internal mutex if required
#[async_trait]
pub trait AddressDeltaObserver: Send + Sync {
    /// Observe a new block
    async fn start_block(&self, block: &BlockInfo);

    /// Observe a delta
    async fn observe_delta(&self, address: &Address, delta: ValueDelta);

    /// Finalise a block
    async fn finalise_block(&self, block: &BlockInfo);
}

/// Immutable UTXO store
/// Note all methods immutable as above
#[async_trait]
pub trait ImmutableUTXOStore: Send + Sync {
    /// Add a UTXO
    async fn add_utxo(&self, key: UTxOIdentifier, value: UTXOValue) -> Result<()>;

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTxOIdentifier) -> Result<()>;

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTxOIdentifier) -> Result<Option<UTXOValue>>;

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize>;
}

/// Ledger state storage
pub struct State {
    /// Last slot number received
    last_slot: u64,

    /// Last block number received
    last_number: u64,

    /// Volatile UTXOs
    volatile_utxos: HashMap<UTxOIdentifier, UTXOValue>,

    /// Index of volatile UTXOs by created block
    volatile_created: VolatileIndex,

    /// Index of volatile UTXOs by spent block
    volatile_spent: VolatileIndex,

    /// Address delta observer
    address_delta_observer: Option<Arc<dyn AddressDeltaObserver>>,

    /// Immutable UTXO store
    immutable_utxos: Arc<dyn ImmutableUTXOStore>,
}

impl State {
    /// Create a new empty state
    pub fn new(immutable_utxo_store: Arc<dyn ImmutableUTXOStore>) -> Self {
        Self {
            last_slot: 0,
            last_number: 0,
            volatile_utxos: HashMap::new(),
            volatile_created: VolatileIndex::new(),
            volatile_spent: VolatileIndex::new(),
            address_delta_observer: None,
            immutable_utxos: immutable_utxo_store,
        }
    }

    /// Register the delta observer
    pub fn register_address_delta_observer(&mut self, observer: Arc<dyn AddressDeltaObserver>) {
        self.address_delta_observer = Some(observer);
    }

    /// Look up a UTXO
    pub async fn lookup_utxo(&self, key: &UTxOIdentifier) -> Result<Option<UTXOValue>> {
        match self.volatile_utxos.get(key) {
            Some(key) => Ok(Some(key.clone())),
            None => Ok(self.immutable_utxos.lookup_utxo(key).await?),
        }
    }

    /// Get the number of valid UTXOs - that is, that have a valid created_at
    /// but no spent_at
    pub async fn count_valid_utxos(&self) -> usize {
        return self.volatile_utxos.len() - self.volatile_spent.len()
            + self.immutable_utxos.len().await.unwrap_or_default();
    }

    /// Observe a block for statistics and handle rollbacks
    pub async fn observe_block(&mut self, block: &BlockInfo) -> Result<()> {
        match block.status {
            BlockStatus::RolledBack => {
                info!(
                    slot = block.slot,
                    number = block.number,
                    "Rollback received"
                );

                // Delete all UTXOs created in or after this block
                let utxos = self.volatile_created.prune_on_or_after(block.number);
                for key in utxos {
                    if let Some(utxo) = self.volatile_utxos.remove(&key) {
                        // Tell the observer to debit it
                        if let Some(observer) = self.address_delta_observer.as_ref() {
                            observer
                                .observe_delta(&utxo.address, -ValueDelta::from(&utxo.value))
                                .await;
                        }
                    }
                }

                // Any remaining (which were necessarily created before this block)
                // that were spent in or after this block can be reinstated
                let utxos = self.volatile_spent.prune_on_or_after(block.number);
                for key in utxos {
                    if let Some(utxo) = self.volatile_utxos.get(&key) {
                        // Tell the observer to recredit it
                        if let Some(observer) = self.address_delta_observer.as_ref() {
                            observer
                                .observe_delta(&utxo.address, ValueDelta::from(&utxo.value))
                                .await;
                        }
                    }
                }

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

        Ok(())
    }

    /// Observe an input UTXO spend
    pub async fn observe_input(&mut self, input: &TxInput, block: &BlockInfo) -> Result<()> {
        let key = input.utxo_identifier;

        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO << {}", key);
        }

        // UTXO exists?
        match self.lookup_utxo(&key).await? {
            Some(utxo) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!(
                        "        - spent {} lovelace from {:?}",
                        utxo.value.coin(),
                        utxo.address
                    );
                }

                // Tell the observer it's spent
                if let Some(observer) = self.address_delta_observer.as_ref() {
                    observer.observe_delta(&utxo.address, -ValueDelta::from(&utxo.value)).await;
                }

                match block.status {
                    BlockStatus::Volatile | BlockStatus::RolledBack => {
                        // Add to volatile spent index
                        self.volatile_spent.add_utxo(&key);
                    }
                    BlockStatus::Bootstrap | BlockStatus::Immutable => {
                        // Immutable - we can delete it immediately
                        self.immutable_utxos.delete_utxo(&key).await?;
                    }
                }
            }
            _ => {
                error!(
                    "UTXO output {} unknown in transaction {} of block {}",
                    &key.output_index(),
                    key.tx_index(),
                    key.block_number()
                );
            }
        }

        Ok(())
    }

    /// Observe an output UXTO creation
    pub async fn observe_output(&mut self, output: &TxOutput, block: &BlockInfo) -> Result<()> {
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("UTXO >> {}", output.utxo_identifier);
            debug!(
                "        - adding {} to {:?}",
                output.value.coin(),
                output.address
            );
        }

        // Insert the UTXO, checking if it already existed
        let key = output.utxo_identifier;

        let value = UTXOValue {
            address: output.address.clone(),
            value: output.value.clone(),
            datum: output.datum.clone(),
        };

        // Add to volatile or immutable maps
        match block.status {
            BlockStatus::Volatile | BlockStatus::RolledBack => {
                self.volatile_created.add_utxo(&key);

                if self.volatile_utxos.insert(key.clone(), value).is_some() {
                    error!(
                        "Saw UTXO {}:{}:{} before",
                        output.utxo_identifier.block_number(),
                        output.utxo_identifier.tx_index(),
                        output.utxo_identifier.output_index(),
                    );
                }
            }
            BlockStatus::Bootstrap | BlockStatus::Immutable => {
                self.immutable_utxos.add_utxo(key.clone(), value).await?;
                // Note we don't check for duplicates in immutable - store
                // may double check this anyway
            }
        };

        // Tell the observer
        if let Some(observer) = self.address_delta_observer.as_ref() {
            observer.observe_delta(&output.address, ValueDelta::from(&output.value)).await;
        }

        Ok(())
    }

    /// Background prune
    async fn prune(&mut self) -> Result<()> {
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
                        self.immutable_utxos.delete_utxo(&key).await?;
                    }
                }
            }

            // Prune the created index too, and transfer the UTXOs to immutable
            let created_utxos = self.volatile_created.prune_before(boundary);
            if !created_utxos.is_empty() {
                info!(
                    "Moving {} volatile UTXOs into immutable",
                    created_utxos.len()
                );
                for key in created_utxos {
                    let value = self.volatile_utxos.remove(&key);
                    if let Some(value) = value {
                        self.immutable_utxos.add_utxo(key, value).await?;
                    }
                }
            }

            self.volatile_utxos.shrink_to_fit();
        }

        Ok(())
    }

    /// Log statistics
    async fn log_stats(&self) {
        let n_immutable = self.immutable_utxos.len().await.unwrap_or_default();
        let n_valid = self.count_valid_utxos().await;
        info!(
            slot = self.last_slot,
            number = self.last_number,
            immutable_utxos = n_immutable,
            volatile_utxos = self.volatile_utxos.len(),
            valid_utxos = n_valid,
        );
    }

    /// Tick for pruning and logging
    pub async fn tick(&mut self) -> Result<()> {
        self.prune().await?;
        self.log_stats().await;
        Ok(())
    }

    /// Handle a message
    pub async fn handle(&mut self, block: &BlockInfo, deltas: &UTXODeltasMessage) -> Result<()> {
        // Start the block for observer
        if let Some(observer) = self.address_delta_observer.as_mut() {
            observer.start_block(&block).await;
        }

        // Observe block for stats and rollbacks
        self.observe_block(&block).await?;

        // Process the deltas
        for delta in &deltas.deltas {
            // UTXODelta

            match delta {
                UTXODelta::Input(tx_input) => {
                    self.observe_input(&tx_input, &block).await?;
                }

                UTXODelta::Output(tx_output) => {
                    self.observe_output(&tx_output, &block).await?;
                }

                _ => {}
            }
        }

        // End the block for observer
        if let Some(observer) = self.address_delta_observer.as_mut() {
            observer.finalise_block(&block).await;
        }

        Ok(())
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryImmutableUTXOStore;
    use acropolis_common::{AssetName, BlockHash, ByronAddress, Era, NativeAsset, Value};
    use config::Config;
    use tokio::sync::Mutex;

    // Create an address for testing - we use Byron just because it's easier to
    // create and test the payload
    fn create_address(n: u8) -> Address {
        Address::Byron(ByronAddress { payload: vec![n] })
    }

    // Create a block for testing
    fn create_block(status: BlockStatus, slot: u64, number: u64) -> BlockInfo {
        BlockInfo {
            status,
            slot,
            number,
            hash: BlockHash::default(),
            epoch: 99,
            epoch_slot: slot,
            new_epoch: false,
            timestamp: slot,
            era: Era::Byron,
        }
    }

    fn new_state() -> State {
        let config = Arc::new(Config::builder().build().unwrap());
        State::new(Arc::new(InMemoryImmutableUTXOStore::new(config)))
    }

    #[tokio::test]
    async fn new_state_is_empty() {
        let state = new_state();
        assert_eq!(0, state.last_slot);
        assert_eq!(0, state.last_number);
        assert_eq!(0, state.count_valid_utxos().await);
        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(0, state.immutable_utxos.len().await.unwrap());
    }

    #[tokio::test]
    async fn observe_output_adds_to_immutable_utxos() {
        let mut state = new_state();
        let datum_data = vec![1, 2, 3, 4, 5];

        let output = TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: create_address(99),
            value: Value::new(
                42,
                vec![(
                    [1u8; 28],
                    vec![
                        NativeAsset {
                            name: AssetName::new(b"TEST").unwrap(),
                            amount: 100,
                        },
                        NativeAsset {
                            name: AssetName::new(b"FOO").unwrap(),
                            amount: 200,
                        },
                    ],
                )],
            ),
            datum: Some(Datum::Inline(datum_data.clone())),
        };

        let block = create_block(BlockStatus::Immutable, 1, 1);
        state.observe_output(&output, &block).await.unwrap();
        assert_eq!(1, state.immutable_utxos.len().await.unwrap());
        assert_eq!(1, state.count_valid_utxos().await);

        let key = output.utxo_identifier;
        match state.lookup_utxo(&key).await.unwrap() {
            Some(value) => {
                assert!(
                    matches!(&value.address, Address::Byron(ByronAddress{ payload })
                if payload[0] == 99)
                );
                assert_eq!(42, value.value.lovelace);

                assert_eq!(1, value.value.assets.len());
                let (policy_id, assets) = &value.value.assets[0];
                assert_eq!([1u8; 28], *policy_id);
                assert_eq!(2, assets.len());

                assert!(assets
                    .iter()
                    .any(|a| a.name == AssetName::new(b"TEST").unwrap() && a.amount == 100));
                assert!(assets
                    .iter()
                    .any(|a| a.name == AssetName::new(b"FOO").unwrap() && a.amount == 200));

                assert!(matches!(
                    value.datum,
                    Some(Datum::Inline(ref data)) if data == &datum_data
                ));
            }

            _ => panic!("UTXO not found"),
        }
    }

    #[tokio::test]
    async fn observe_input_spends_utxo() {
        let mut state = new_state();
        let output = TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: create_address(99),
            value: Value::new(
                42,
                vec![(
                    [1u8; 28],
                    vec![
                        NativeAsset {
                            name: AssetName::new(b"TEST").unwrap(),
                            amount: 100,
                        },
                        NativeAsset {
                            name: AssetName::new(b"FOO").unwrap(),
                            amount: 200,
                        },
                    ],
                )],
            ),
            datum: None,
        };

        let block1 = create_block(BlockStatus::Immutable, 1, 1);
        state.observe_output(&output, &block1).await.unwrap();
        assert_eq!(1, state.immutable_utxos.len().await.unwrap());
        assert_eq!(1, state.count_valid_utxos().await);

        let input = TxInput {
            utxo_identifier: UTxOIdentifier::new(
                output.utxo_identifier.block_number(),
                output.utxo_identifier.tx_index(),
                output.utxo_identifier.output_index(),
            ),
        };

        let block2 = create_block(BlockStatus::Immutable, 2, 2);
        state.observe_input(&input, &block2).await.unwrap();
        assert_eq!(0, state.immutable_utxos.len().await.unwrap());
        assert_eq!(0, state.count_valid_utxos().await);
    }

    #[tokio::test]
    async fn rollback_removes_future_created_utxos() {
        let mut state = new_state();
        let output = TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: create_address(99),
            value: Value::new(
                42,
                vec![(
                    [1u8; 28],
                    vec![
                        NativeAsset {
                            name: AssetName::new(b"TEST").unwrap(),
                            amount: 100,
                        },
                        NativeAsset {
                            name: AssetName::new(b"FOO").unwrap(),
                            amount: 200,
                        },
                    ],
                )],
            ),
            datum: None,
        };

        let block10 = create_block(BlockStatus::Volatile, 10, 10);
        state.observe_block(&block10).await.unwrap();
        state.observe_output(&output, &block10).await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos().await);

        let block9 = create_block(BlockStatus::RolledBack, 9, 9);
        state.observe_block(&block9).await.unwrap();

        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos().await);
    }

    #[tokio::test]
    async fn rollback_reinstates_future_spent_utxos() {
        let mut state = new_state();

        // Create the UTXO in block 10
        let output = TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: create_address(99),
            value: Value::new(
                42,
                vec![(
                    [1u8; 28],
                    vec![
                        NativeAsset {
                            name: AssetName::new(b"TEST").unwrap(),
                            amount: 100,
                        },
                        NativeAsset {
                            name: AssetName::new(b"FOO").unwrap(),
                            amount: 200,
                        },
                    ],
                )],
            ),
            datum: None,
        };

        let block10 = create_block(BlockStatus::Volatile, 10, 10);
        state.observe_block(&block10).await.unwrap();
        state.observe_output(&output, &block10).await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos().await);

        // Spend it in block 11
        let input = TxInput {
            utxo_identifier: UTxOIdentifier::new(
                output.utxo_identifier.block_number(),
                output.utxo_identifier.tx_index(),
                output.utxo_identifier.output_index(),
            ),
        };

        let block11 = create_block(BlockStatus::Volatile, 11, 11);
        state.observe_block(&block11).await.unwrap();
        state.observe_input(&input, &block11).await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos().await);

        // Roll back to 11
        let block11_2 = create_block(BlockStatus::RolledBack, 11, 11);
        state.observe_block(&block11_2).await.unwrap();

        // Should be reinstated
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos().await);
    }

    #[tokio::test]
    async fn prune_shifts_new_utxos_into_immutable() {
        let mut state = new_state();
        let output = TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: create_address(99),
            value: Value::new(
                42,
                vec![(
                    [1u8; 28],
                    vec![
                        NativeAsset {
                            name: AssetName::new(b"TEST").unwrap(),
                            amount: 100,
                        },
                        NativeAsset {
                            name: AssetName::new(b"FOO").unwrap(),
                            amount: 200,
                        },
                    ],
                )],
            ),
            datum: None,
        };

        let block1 = create_block(BlockStatus::Volatile, 1, 1);
        state.observe_block(&block1).await.unwrap();
        state.observe_output(&output, &block1).await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.immutable_utxos.len().await.unwrap());
        assert_eq!(1, state.count_valid_utxos().await);

        // Prune shouldn't do anything yet
        state.prune().await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.immutable_utxos.len().await.unwrap());

        // Observe a block much later
        let block = create_block(BlockStatus::Volatile, 23492, 5483);
        state.observe_block(&block).await.unwrap();
        assert_eq!(5483, state.last_number);

        state.prune().await.unwrap();
        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(1, state.immutable_utxos.len().await.unwrap());
    }

    #[tokio::test]
    async fn prune_deletes_old_spent_utxos() {
        let mut state = new_state();
        let output = TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: create_address(99),
            value: Value::new(
                42,
                vec![(
                    [1u8; 28],
                    vec![
                        NativeAsset {
                            name: AssetName::new(b"TEST").unwrap(),
                            amount: 100,
                        },
                        NativeAsset {
                            name: AssetName::new(b"FOO").unwrap(),
                            amount: 200,
                        },
                    ],
                )],
            ),
            datum: None,
        };

        let block1 = create_block(BlockStatus::Volatile, 1, 1);
        state.observe_block(&block1).await.unwrap();
        state.observe_output(&output, &block1).await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos().await);

        let input = TxInput {
            utxo_identifier: UTxOIdentifier::new(
                output.utxo_identifier.block_number(),
                output.utxo_identifier.tx_index(),
                output.utxo_identifier.output_index(),
            ),
        };

        let block2 = create_block(BlockStatus::Volatile, 2, 2);
        state.observe_block(&block2).await.unwrap();
        state.observe_input(&input, &block2).await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos().await);

        // Prune shouldn't do anything yet
        state.prune().await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());

        // Observe a block much later
        let block = create_block(BlockStatus::Volatile, 23492, 5483);
        state.observe_block(&block).await.unwrap();
        assert_eq!(5483, state.last_number);

        state.prune().await.unwrap();
        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(0, state.immutable_utxos.len().await.unwrap());
    }

    struct TestDeltaObserver {
        balance: Mutex<i64>,
        asset_balances: Mutex<HashMap<([u8; 28], AssetName), i64>>,
    }

    impl TestDeltaObserver {
        fn new() -> Self {
            Self {
                balance: Mutex::new(0),
                asset_balances: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl AddressDeltaObserver for TestDeltaObserver {
        async fn start_block(&self, _block: &BlockInfo) {}
        async fn observe_delta(&self, address: &Address, delta: ValueDelta) {
            assert!(matches!(
                &address,
                Address::Byron(ByronAddress { payload }) if payload[0] == 99
            ));
            assert!(delta.lovelace == 42 || delta.lovelace == -42);

            let mut balance = self.balance.lock().await;
            *balance += delta.lovelace;

            let mut asset_balances = self.asset_balances.lock().await;
            for (policy, assets) in &delta.assets {
                assert_eq!([1u8; 28], *policy);
                for asset in assets {
                    assert!(
                        (asset.name == AssetName::new(b"TEST").unwrap()
                            && (asset.amount == 100 || asset.amount == -100))
                            || (asset.name == AssetName::new(b"FOO").unwrap()
                                && (asset.amount == 200 || asset.amount == -200))
                    );
                    let key = (*policy, asset.name.clone());
                    *asset_balances.entry(key).or_insert(0) += asset.amount;
                }
            }
        }

        async fn finalise_block(&self, _block: &BlockInfo) {}
    }

    #[tokio::test]
    async fn observe_output_then_input_notifies_net_0_balance_change() {
        let mut state = new_state();
        let observer = Arc::new(TestDeltaObserver::new());
        state.register_address_delta_observer(observer.clone());

        let output = TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: create_address(99),
            value: Value::new(
                42,
                vec![(
                    [1u8; 28],
                    vec![
                        NativeAsset {
                            name: AssetName::new(b"TEST").unwrap(),
                            amount: 100,
                        },
                        NativeAsset {
                            name: AssetName::new(b"FOO").unwrap(),
                            amount: 200,
                        },
                    ],
                )],
            ),
            datum: None,
        };

        let block1 = create_block(BlockStatus::Immutable, 1, 1);
        state.observe_output(&output, &block1).await.unwrap();
        assert_eq!(1, state.immutable_utxos.len().await.unwrap());
        assert_eq!(1, state.count_valid_utxos().await);
        assert_eq!(42, *observer.balance.lock().await);

        let input = TxInput {
            utxo_identifier: UTxOIdentifier::new(
                output.utxo_identifier.block_number(),
                output.utxo_identifier.tx_index(),
                output.utxo_identifier.output_index(),
            ),
        };

        let block2 = create_block(BlockStatus::Immutable, 2, 2);
        state.observe_input(&input, &block2).await.unwrap();
        assert_eq!(0, state.immutable_utxos.len().await.unwrap());
        assert_eq!(0, state.count_valid_utxos().await);
        assert_eq!(0, *observer.balance.lock().await);
        let ab = observer.asset_balances.lock().await;
        assert_eq!(
            *ab.get(&([1u8; 28], AssetName::new(b"TEST").unwrap())).unwrap(),
            0
        );
        assert_eq!(
            *ab.get(&([1u8; 28], AssetName::new(b"FOO").unwrap())).unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn observe_rollback_notifies_balance_debit_on_future_created_utxos() {
        let mut state = new_state();
        let observer = Arc::new(TestDeltaObserver::new());
        state.register_address_delta_observer(observer.clone());

        let output = TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: create_address(99),
            value: Value::new(
                42,
                vec![(
                    [1u8; 28],
                    vec![
                        NativeAsset {
                            name: AssetName::new(b"TEST").unwrap(),
                            amount: 100,
                        },
                        NativeAsset {
                            name: AssetName::new(b"FOO").unwrap(),
                            amount: 200,
                        },
                    ],
                )],
            ),
            datum: None,
        };

        let block10 = create_block(BlockStatus::Volatile, 10, 10);
        state.observe_block(&block10).await.unwrap();
        state.observe_output(&output, &block10).await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos().await);
        assert_eq!(42, *observer.balance.lock().await);

        let block9 = create_block(BlockStatus::RolledBack, 200, 9);
        state.observe_block(&block9).await.unwrap();

        assert_eq!(0, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos().await);
        assert_eq!(0, *observer.balance.lock().await);

        let ab = observer.asset_balances.lock().await;
        assert_eq!(
            *ab.get(&([1u8; 28], AssetName::new(b"TEST").unwrap())).unwrap(),
            0
        );
        assert_eq!(
            *ab.get(&([1u8; 28], AssetName::new(b"FOO").unwrap())).unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn observe_rollback_notifies_balance_credit_on_future_spent_utxos() {
        let mut state = new_state();
        let observer = Arc::new(TestDeltaObserver::new());
        state.register_address_delta_observer(observer.clone());

        // Create the UTXO in block 10
        let output = TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: create_address(99),
            value: Value::new(
                42,
                vec![(
                    [1u8; 28],
                    vec![
                        NativeAsset {
                            name: AssetName::new(b"TEST").unwrap(),
                            amount: 100,
                        },
                        NativeAsset {
                            name: AssetName::new(b"FOO").unwrap(),
                            amount: 200,
                        },
                    ],
                )],
            ),
            datum: None,
        };

        let block10 = create_block(BlockStatus::Volatile, 10, 10);
        state.observe_block(&block10).await.unwrap();
        state.observe_output(&output, &block10).await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos().await);
        assert_eq!(42, *observer.balance.lock().await);

        // Spend it in block 11
        let input = TxInput {
            utxo_identifier: UTxOIdentifier::new(
                output.utxo_identifier.block_number(),
                output.utxo_identifier.tx_index(),
                output.utxo_identifier.output_index(),
            ),
        };

        let block11 = create_block(BlockStatus::Volatile, 11, 11);
        state.observe_block(&block11).await.unwrap();
        state.observe_input(&input, &block11).await.unwrap();
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(0, state.count_valid_utxos().await);
        assert_eq!(0, *observer.balance.lock().await);

        // Roll back to 11
        let block11_2 = create_block(BlockStatus::RolledBack, 200, 11);
        state.observe_block(&block11_2).await.unwrap();

        // Should be reinstated
        assert_eq!(1, state.volatile_utxos.len());
        assert_eq!(1, state.count_valid_utxos().await);
        assert_eq!(42, *observer.balance.lock().await);

        let ab = observer.asset_balances.lock().await;
        assert_eq!(
            *ab.get(&([1u8; 28], AssetName::new(b"TEST").unwrap())).unwrap(),
            100
        );
        assert_eq!(
            *ab.get(&([1u8; 28], AssetName::new(b"FOO").unwrap())).unwrap(),
            200
        );
    }
}
