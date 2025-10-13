use std::collections::HashMap;

use acropolis_common::KeyHash;
use fjall::{Config, Keyspace, PartitionCreateOptions};

const POOL_ID_LEN: usize = 28;
const STAKE_KEY_LEN: usize = 28;
const EPOCH_LEN: usize = 8;
const TOTAL_KEY_LEN: usize = EPOCH_LEN + POOL_ID_LEN + STAKE_KEY_LEN; // 64 bytes
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
}

impl SPDDStore {
    pub fn new(path: impl AsRef<std::path::Path>) -> fjall::Result<Self> {
        let path = path.as_ref();

        // Delete existing data
        if path.exists() {
            std::fs::remove_dir_all(path).map_err(|e| fjall::Error::Io(e))?;
        }

        let keyspace = Config::new(path).open()?;
        let spdd = keyspace.open_partition("spdd", PartitionCreateOptions::default())?;

        Ok(Self { keyspace, spdd })
    }

    /// Store SPDD state for an epoch
    pub fn store_spdd(
        &self,
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

        Ok(())
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
        let spdd_store =
            SPDDStore::new(std::path::Path::new(DB_PATH)).expect("Failed to create SPDD store");
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
}
