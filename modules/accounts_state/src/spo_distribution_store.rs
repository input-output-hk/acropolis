use std::collections::HashMap;

use acropolis_common::{AddrKeyhash, PoolId};
use anyhow::Result;
use fjall::{Config, Keyspace, PartitionCreateOptions};

const POOL_ID_LEN: usize = 28;
const STAKE_KEY_LEN: usize = 28;
const EPOCH_LEN: usize = 8;
const TOTAL_KEY_LEN: usize = EPOCH_LEN + POOL_ID_LEN + STAKE_KEY_LEN;

// Batch size balances commit overhead vs memory usage
// ~720KB per batch (72 bytes × 10,000)
// ~130 commits for typical epoch (~1.3M delegations)
const BATCH_SIZE: usize = 10_000;

fn encode_key(epoch: u64, pool_id: &PoolId, stake_key: &AddrKeyhash) -> Vec<u8> {
    let mut key = Vec::with_capacity(TOTAL_KEY_LEN);
    key.extend_from_slice(&epoch.to_be_bytes());
    key.extend_from_slice(pool_id);
    key.extend_from_slice(stake_key);
    key
}

fn encode_epoch_pool_prefix(epoch: u64, pool_id: &PoolId) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(EPOCH_LEN + POOL_ID_LEN);
    prefix.extend_from_slice(&epoch.to_be_bytes());
    prefix.extend_from_slice(pool_id);
    prefix
}

fn decode_key(key: &[u8]) -> Result<(u64, PoolId, AddrKeyhash)> {
    let epoch = u64::from_be_bytes(key[..EPOCH_LEN].try_into()?);
    let pool_id = key[EPOCH_LEN..EPOCH_LEN + POOL_ID_LEN].to_vec();
    let stake_key = key[EPOCH_LEN + POOL_ID_LEN..].to_vec();
    Ok((epoch, pool_id, stake_key))
}

/// Encode epoch completion marker key
fn encode_epoch_marker(epoch: u64) -> Vec<u8> {
    epoch.to_be_bytes().to_vec()
}

pub struct SPDDStore {
    keyspace: Keyspace,
    /// Partition for all SPDD data
    /// Key format: epoch(8 bytes) + pool_id + stake_key
    /// Value: amount(8 bytes)
    spdd: fjall::PartitionHandle,
    /// Partition for epoch completion markers
    /// Key format: epoch(8 bytes)
    /// Value: "complete"
    epoch_markers: fjall::PartitionHandle,
    /// Maximum number of epochs to retain (None = unlimited)
    retention_epochs: Option<u64>,
}

impl SPDDStore {
    #[allow(dead_code)]
    pub fn new(
        path: impl AsRef<std::path::Path>,
        retention_epochs: Option<u64>,
    ) -> fjall::Result<Self> {
        let path = path.as_ref();
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }

        let keyspace = Config::new(path).open()?;
        let spdd = keyspace.open_partition("spdd", PartitionCreateOptions::default())?;
        let epoch_markers =
            keyspace.open_partition("epoch_markers", PartitionCreateOptions::default())?;

        Ok(Self {
            keyspace,
            spdd,
            epoch_markers,
            retention_epochs,
        })
    }

    pub fn load(
        path: impl AsRef<std::path::Path>,
        retention_epochs: Option<u64>,
    ) -> fjall::Result<Self> {
        let path = path.as_ref();

        let keyspace = Config::new(path).open()?;
        let spdd = keyspace.open_partition("spdd", PartitionCreateOptions::default())?;
        let epoch_markers =
            keyspace.open_partition("epoch_markers", PartitionCreateOptions::default())?;

        Ok(Self {
            keyspace,
            spdd,
            epoch_markers,
            retention_epochs,
        })
    }

    pub fn is_epoch_complete(&self, epoch: u64) -> fjall::Result<bool> {
        let marker_key = encode_epoch_marker(epoch);
        Ok(matches!(self.epoch_markers.get(&marker_key)?, Some(value) if value.eq(b"complete")))
    }

    pub fn store_spdd(
        &mut self,
        epoch: u64,
        spdd_state: HashMap<PoolId, Vec<(AddrKeyhash, u64)>>,
    ) -> fjall::Result<()> {
        if self.is_epoch_complete(epoch)? {
            return Ok(());
        }
        self.remove_epoch_data(epoch)?;

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
        if count > 0 {
            batch.commit()?;
        }

        // Mark epoch as complete (single key operation)
        let marker_key = encode_epoch_marker(epoch);
        self.epoch_markers.insert(marker_key, b"complete")?;

        if let Some(retention) = self.retention_epochs {
            if epoch >= retention {
                let keep_from_epoch = epoch - retention + 1;
                self.prune_epochs_before(keep_from_epoch)?;
            }
        }

        Ok(())
    }

    pub fn remove_epoch_data(&self, epoch: u64) -> fjall::Result<u64> {
        // Remove epoch marker first - if process fails midway, epoch will be marked incomplete
        let marker_key = encode_epoch_marker(epoch);
        self.epoch_markers.remove(marker_key)?;

        let prefix = epoch.to_be_bytes();
        let mut batch = self.keyspace.batch();
        let mut deleted_count = 0;
        let mut total_deleted_count: u64 = 0;

        for item in self.spdd.prefix(prefix) {
            let (key, _) = item?;
            batch.remove(&self.spdd, key);
            total_deleted_count += 1;

            deleted_count += 1;
            if deleted_count >= BATCH_SIZE {
                batch.commit()?;
                batch = self.keyspace.batch();
                deleted_count = 0;
            }
        }

        if deleted_count > 0 {
            batch.commit()?;
        }

        Ok(total_deleted_count)
    }

    pub fn prune_epochs_before(&self, before_epoch: u64) -> fjall::Result<u64> {
        let mut deleted_epochs: u64 = 0;

        for epoch in (0..before_epoch).rev() {
            let deleted_count = self.remove_epoch_data(epoch)?;
            if deleted_count == 0 {
                break;
            }
            deleted_epochs += 1;
        }
        Ok(deleted_epochs)
    }

    pub fn query_by_epoch(&self, epoch: u64) -> Result<Vec<(PoolId, AddrKeyhash, u64)>> {
        if !self.is_epoch_complete(epoch)? {
            return Err(anyhow::anyhow!("Epoch SPDD Data is not complete"));
        }

        let prefix = epoch.to_be_bytes();
        let mut result = Vec::new();
        for item in self.spdd.prefix(prefix) {
            let (key, value) = item?;
            let (_, pool_id, stake_key) = decode_key(&key)?;
            let amount = u64::from_be_bytes(value.as_ref().try_into()?);
            result.push((pool_id, stake_key, amount));
        }
        Ok(result)
    }

    pub fn query_by_epoch_and_pool(
        &self,
        epoch: u64,
        pool_id: &PoolId,
    ) -> Result<Vec<(AddrKeyhash, u64)>> {
        if !self.is_epoch_complete(epoch)? {
            return Err(anyhow::anyhow!("Epoch SPDD Data is not complete"));
        }

        let prefix = encode_epoch_pool_prefix(epoch, pool_id);
        let mut result = Vec::new();
        for item in self.spdd.prefix(prefix) {
            let (key, value) = item?;
            let (_, _, stake_key) = decode_key(&key)?;
            let amount = u64::from_be_bytes(value.as_ref().try_into()?);
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
        let mut spdd_state: HashMap<PoolId, Vec<(AddrKeyhash, u64)>> = HashMap::new();
        spdd_state.insert(
            vec![0x01; 28],
            vec![(vec![0x10; 28], 100), (vec![0x11; 28], 150)],
        );
        spdd_state.insert(
            vec![0x02; 28],
            vec![(vec![0x20; 28], 200), (vec![0x21; 28], 250)],
        );
        assert!(spdd_store.store_spdd(1, spdd_state).is_ok());

        let result = spdd_store.query_by_epoch(1).unwrap();
        assert_eq!(result.len(), 4);
        let result = spdd_store.query_by_epoch_and_pool(1, &vec![0x01; 28]).unwrap();
        assert_eq!(result.len(), 2);
        let result = spdd_store.query_by_epoch_and_pool(1, &vec![0x02; 28]).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_prune_old_epochs() {
        let mut spdd_store = SPDDStore::new(std::path::Path::new("spdd_prune_test_db"), Some(2))
            .expect("Failed to create SPDD store");

        for epoch in 1..=3 {
            let mut spdd_state: HashMap<PoolId, Vec<(AddrKeyhash, u64)>> = HashMap::new();
            spdd_state.insert(
                vec![epoch as u8; 28],
                vec![(vec![0x10; 28], epoch * 100), (vec![0x11; 28], epoch * 150)],
            );
            spdd_store.store_spdd(epoch, spdd_state).expect("Failed to store SPDD state");
        }

        assert!(!spdd_store.is_epoch_complete(1).unwrap());
        assert!(spdd_store.is_epoch_complete(2).unwrap());
        assert!(spdd_store.is_epoch_complete(3).unwrap());

        assert!(spdd_store.query_by_epoch(1).is_err());
        let result = spdd_store.query_by_epoch(2).unwrap();
        assert_eq!(result.len(), 2);
        let result = spdd_store.query_by_epoch(3).unwrap();
        assert_eq!(result.len(), 2);
    }
}
