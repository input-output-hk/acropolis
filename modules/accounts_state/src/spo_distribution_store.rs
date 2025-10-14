use std::collections::HashMap;

use acropolis_common::KeyHash;
use fjall::{Config, Keyspace, PartitionCreateOptions};

const POOL_ID_LEN: usize = 28;
const STAKE_KEY_LEN: usize = 28;
const EPOCH_LEN: usize = 8;
const TOTAL_KEY_LEN: usize = EPOCH_LEN + POOL_ID_LEN + STAKE_KEY_LEN; // 64 bytes

// Batch size balances commit overhead vs memory usage
// ~720KB per batch (72 bytes × 10,000)
// ~130 commits for typical epoch (~1.3M delegations)
const BATCH_SIZE: usize = 10_000;

/// Encode: epoch + pool_id + stake_key
fn encode_key(epoch: u64, pool_id: &KeyHash, stake_key: &KeyHash) -> Vec<u8> {
    let mut key = Vec::with_capacity(TOTAL_KEY_LEN);
    key.extend_from_slice(&epoch.to_be_bytes());
    key.extend_from_slice(pool_id);
    key.extend_from_slice(stake_key);
    key
}

/// Encode: epoch + pool_id (for prefix queries)
fn encode_epoch_pool_prefix(epoch: u64, pool_id: &KeyHash) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(EPOCH_LEN + POOL_ID_LEN);
    prefix.extend_from_slice(&epoch.to_be_bytes());
    prefix.extend_from_slice(pool_id);
    prefix
}

/// Decode key to extract pool_id and stake_key
fn decode_key(key: &[u8]) -> (KeyHash, KeyHash) {
    let pool_id = key[EPOCH_LEN..EPOCH_LEN + POOL_ID_LEN].to_vec();
    let stake_key = key[EPOCH_LEN + POOL_ID_LEN..].to_vec();
    (pool_id, stake_key)
}

pub struct SPDDStore {
    keyspace: Keyspace,
    /// Single partition for all SPDD data
    /// Key format: epoch(8 bytes) + pool_id + stake_key
    /// Value: amount(8 bytes)
    spdd: fjall::PartitionHandle,
    /// Maximum number of epochs to retain (None = unlimited)
    retention_epochs: Option<u64>,
    /// Latest epoch stored
    latest_epoch: Option<u64>,
}

impl SPDDStore {
    pub fn new(
        path: impl AsRef<std::path::Path>,
        retention_epochs: Option<u64>,
    ) -> fjall::Result<Self> {
        let path = path.as_ref();

        let keyspace = Config::new(path).open()?;
        let spdd = keyspace.open_partition("spdd", PartitionCreateOptions::default())?;

        Ok(Self {
            keyspace,
            spdd,
            retention_epochs,
            latest_epoch: None,
        })
    }

    /// Store SPDD state for an epoch and prune old epochs if needed
    pub fn store_spdd(
        &mut self,
        epoch: u64,
        spdd_state: HashMap<KeyHash, Vec<(KeyHash, u64)>>,
    ) -> fjall::Result<()> {
        let mut batch = self.keyspace.batch();
        let mut count = 0;

        for (pool_id, delegations) in spdd_state {
            for (stake_key, amount) in delegations {
                let key = encode_key(epoch, &pool_id, &stake_key);
                let value = amount.to_be_bytes();
                batch.insert(&self.spdd, key, value);

                count += 1;
                if count >= BATCH_SIZE {
                    batch.commit()?;
                    batch = self.keyspace.batch();
                    count = 0;
                }
            }
        }

        // Commit remaining entries
        if count > 0 {
            batch.commit()?;
        }

        self.latest_epoch = Some(epoch);

        // Prune old epochs if retention is configured
        // Keep the last N epochs, delete everything older
        if let Some(retention) = self.retention_epochs {
            if epoch >= retention {
                // Keep epochs [epoch - retention + 1, epoch]
                // Delete everything before (epoch - retention + 1)
                let keep_from_epoch = epoch - retention + 1;
                self.prune_epochs_before(keep_from_epoch)?;
            }
        }

        Ok(())
    }

    /// Prune all SPDD data for epochs before the specified epoch
    pub fn prune_epochs_before(&self, before_epoch: u64) -> fjall::Result<()> {
        let mut batch = self.keyspace.batch();
        let mut deleted_count = 0;

        // Iterate through all epochs less than before_epoch
        for epoch in 0..before_epoch {
            let prefix = epoch.to_be_bytes();

            for item in self.spdd.prefix(prefix) {
                let (key, _) = item?;
                batch.remove(&self.spdd, key);

                deleted_count += 1;
                if deleted_count >= BATCH_SIZE {
                    batch.commit()?;
                    batch = self.keyspace.batch();
                    deleted_count = 0;
                }
            }
        }

        // Commit remaining deletions
        if deleted_count > 0 {
            batch.commit()?;
        }

        Ok(())
    }

    pub fn is_epoch_stored(&self, epoch: u64) -> Option<bool> {
        let Some(latest_epoch) = self.latest_epoch else {
            return None;
        };
        let min_epoch = match self.retention_epochs {
            Some(retention) => {
                if latest_epoch > retention {
                    latest_epoch - retention + 1
                } else {
                    0
                }
            }
            None => 0,
        };

        Some(epoch >= min_epoch && epoch <= latest_epoch)
    }

    /// Query all data for an epoch
    /// Returns: Vec<(PoolId, StakeKey, ActiveStakeAmount)>
    pub fn query_by_epoch(&self, epoch: u64) -> fjall::Result<Vec<(KeyHash, KeyHash, u64)>> {
        let prefix = epoch.to_be_bytes();
        let mut result = Vec::new();

        for item in self.spdd.prefix(prefix) {
            let (key, value) = item?;
            let (pool_id, stake_key) = decode_key(&key);
            let amount = u64::from_be_bytes(value.as_ref().try_into().unwrap());
            result.push((pool_id, stake_key, amount));
        }

        Ok(result)
    }

    /// Query by epoch and pool_id
    /// Returns: Vec<(StakeKey, ActiveStakeAmount)>
    pub fn query_by_epoch_and_pool(
        &self,
        epoch: u64,
        pool_id: &KeyHash,
    ) -> fjall::Result<Vec<(KeyHash, u64)>> {
        let prefix = encode_epoch_pool_prefix(epoch, pool_id);
        let mut result = Vec::new();

        for item in self.spdd.prefix(prefix) {
            let (key, value) = item?;
            let stake_key = key[EPOCH_LEN + POOL_ID_LEN..].to_vec();
            let amount = u64::from_be_bytes(value.as_ref().try_into().unwrap());
            result.push((stake_key, amount));
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DB_PATH: &str = "spdd_db";

    #[test]
    fn test_store_spdd_state() {
        let mut spdd_store = SPDDStore::new(std::path::Path::new(DB_PATH), None)
            .expect("Failed to create SPDD store");
        let mut spdd_state: HashMap<KeyHash, Vec<(KeyHash, u64)>> = HashMap::new();
        spdd_state.insert(
            vec![0x01; 28],
            vec![(vec![0x10; 28], 100), (vec![0x11; 28], 150)],
        );
        spdd_state.insert(
            vec![0x02; 28],
            vec![(vec![0x20; 28], 200), (vec![0x21; 28], 250)],
        );
        spdd_store.store_spdd(1, spdd_state).expect("Failed to store SPDD state");

        let result = spdd_store.query_by_epoch(1).expect("Failed to query SPDD state");
        assert_eq!(result.len(), 4);
        let result = spdd_store
            .query_by_epoch_and_pool(1, &vec![0x01; 28])
            .expect("Failed to query SPDD state");
        assert_eq!(result.len(), 2);
        let result = spdd_store
            .query_by_epoch_and_pool(1, &vec![0x02; 28])
            .expect("Failed to query SPDD state");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_prune_old_epochs() {
        let mut spdd_store = SPDDStore::new(std::path::Path::new("spdd_prune_test_db"), Some(2))
            .expect("Failed to create SPDD store");

        // Store data for epochs 1, 2, 3
        for epoch in 1..=3 {
            let mut spdd_state: HashMap<KeyHash, Vec<(KeyHash, u64)>> = HashMap::new();
            spdd_state.insert(
                vec![epoch as u8; 28],
                vec![(vec![0x10; 28], epoch * 100), (vec![0x11; 28], epoch * 150)],
            );
            spdd_store.store_spdd(epoch, spdd_state).expect("Failed to store SPDD state");
        }

        // After storing epoch 3 with retention=2, epoch 1 should be pruned
        assert!(!spdd_store.is_epoch_stored(1).unwrap());
        assert!(spdd_store.is_epoch_stored(2).unwrap());
        assert!(spdd_store.is_epoch_stored(3).unwrap());

        let result = spdd_store.query_by_epoch(1);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0, "Epoch 1 should be pruned");

        let result = spdd_store.query_by_epoch(2).expect("Failed to query epoch 2");
        assert_eq!(result.len(), 2, "Epoch 2 should still exist");

        let result = spdd_store.query_by_epoch(3).expect("Failed to query epoch 3");
        assert_eq!(result.len(), 2, "Epoch 3 should still exist");
    }
}
