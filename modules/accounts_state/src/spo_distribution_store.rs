use acropolis_common::{PoolId, StakeAddress};
use anyhow::{anyhow, Context, Result};
use fjall::{Config, Keyspace, PartitionCreateOptions};
use std::collections::HashMap;

const POOL_ID_LENGTH: usize = 28;
const STAKE_ADDRESS_LEN: usize = 29; // 1 byte header + 28 bytes hash
const EPOCH_LEN: usize = 8;
const TOTAL_KEY_LEN: usize = EPOCH_LEN + POOL_ID_LENGTH + STAKE_ADDRESS_LEN;

// Batch size balances commit overhead vs memory usage
// ~720KB per batch (72 bytes × 10,000)
// ~130 commits for typical epoch (~1.3M delegations)
const BATCH_SIZE: usize = 10_000;

fn encode_key(epoch: u64, pool_id: &PoolId, stake_address: &StakeAddress) -> Vec<u8> {
    let mut key = Vec::with_capacity(TOTAL_KEY_LEN);
    key.extend_from_slice(&epoch.to_be_bytes());
    key.extend_from_slice(pool_id.as_ref());
    key.extend_from_slice(&stake_address.to_binary());

    key
}

fn encode_epoch_pool_prefix(epoch: u64, pool_id: &PoolId) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(EPOCH_LEN + POOL_ID_LENGTH);
    prefix.extend_from_slice(&epoch.to_be_bytes());
    prefix.extend_from_slice(pool_id.as_ref());
    prefix
}

fn decode_key(key: &[u8]) -> Result<(u64, PoolId, StakeAddress)> {
    let epoch_bytes: [u8; EPOCH_LEN] = key[..EPOCH_LEN]
        .try_into()
        .map_err(|_| anyhow!("Failed to extract epoch bytes (offset 0-{})", EPOCH_LEN))?;
    let epoch = u64::from_be_bytes(epoch_bytes);

    let pool_id: PoolId = key[EPOCH_LEN..EPOCH_LEN + POOL_ID_LENGTH].try_into().map_err(|_| {
        anyhow!(
            "Failed to extract pool ID bytes (offset {}-{})",
            EPOCH_LEN,
            EPOCH_LEN + POOL_ID_LENGTH
        )
    })?;

    let stake_address_bytes = &key[EPOCH_LEN + POOL_ID_LENGTH..];
    let stake_address = StakeAddress::from_binary(stake_address_bytes).with_context(|| {
        format!(
            "Failed to decode stake address from {} bytes at offset {}",
            stake_address_bytes.len(),
            EPOCH_LEN + POOL_ID_LENGTH
        )
    })?;

    Ok((epoch, pool_id, stake_address))
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
    retention_epochs: u64,
}

impl SPDDStore {
    #[allow(dead_code)]
    pub fn new(path: impl AsRef<std::path::Path>, retention_epochs: u64) -> fjall::Result<Self> {
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

    pub fn load(path: impl AsRef<std::path::Path>, retention_epochs: u64) -> fjall::Result<Self> {
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
        spdd_state: HashMap<PoolId, Vec<(StakeAddress, u64)>>,
    ) -> fjall::Result<()> {
        if self.is_epoch_complete(epoch)? {
            return Ok(());
        }
        self.remove_epoch_data(epoch)?;

        let mut batch = self.keyspace.batch();
        let mut count = 0;
        for (pool_id, delegations) in spdd_state {
            for (stake_address, amount) in delegations {
                let key = encode_key(epoch, &pool_id, &stake_address);
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

        if epoch >= self.retention_epochs {
            let keep_from_epoch = epoch - self.retention_epochs + 1;
            self.prune_epochs_before(keep_from_epoch)?;
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

    pub fn query_by_epoch(&self, epoch: u64) -> Result<Vec<(PoolId, StakeAddress, u64)>> {
        if !self.is_epoch_complete(epoch)? {
            return Err(anyhow::anyhow!("Epoch SPDD Data is not complete"));
        }

        let prefix = epoch.to_be_bytes();
        let mut result = Vec::new();
        for item in self.spdd.prefix(prefix) {
            let (key, value) = item?;
            let (_, pool_id, stake_address) = decode_key(&key)?;
            let amount = u64::from_be_bytes(value.as_ref().try_into()?);
            result.push((pool_id, stake_address, amount));
        }
        Ok(result)
    }

    pub fn query_by_epoch_and_pool(
        &self,
        epoch: u64,
        pool_id: &PoolId,
    ) -> Result<Vec<(StakeAddress, u64)>> {
        if !self.is_epoch_complete(epoch)? {
            return Err(anyhow::anyhow!("Epoch SPDD Data is not complete"));
        }

        let prefix = encode_epoch_pool_prefix(epoch, pool_id);
        let mut result = Vec::new();
        for item in self.spdd.prefix(prefix) {
            let (key, value) = item?;
            let (_, _, stake_address) = decode_key(&key)?;
            let amount = u64::from_be_bytes(value.as_ref().try_into()?);
            result.push((stake_address, amount));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::crypto::keyhash_224;
    use acropolis_common::NetworkId::Mainnet;
    use acropolis_common::{PoolId, StakeCredential};
    use tempfile::TempDir;

    fn test_pool_hash(byte: u8) -> PoolId {
        keyhash_224(&[byte]).into()
    }

    fn test_stake_address(byte: u8) -> StakeAddress {
        StakeAddress::new(StakeCredential::AddrKeyHash(keyhash_224(&[byte])), Mainnet)
    }

    #[test]
    fn test_store_and_query_spdd() {
        let temp_dir = TempDir::new().unwrap();
        let mut spdd_store =
            SPDDStore::new(temp_dir.path(), 10).expect("Failed to create SPDD store");

        let mut spdd_state: HashMap<PoolId, Vec<(StakeAddress, u64)>> = HashMap::new();
        spdd_state.insert(
            test_pool_hash(0x01),
            vec![
                (test_stake_address(0x10), 100),
                (test_stake_address(0x11), 150),
            ],
        );
        spdd_state.insert(
            test_pool_hash(0x02),
            vec![
                (test_stake_address(0x20), 200),
                (test_stake_address(0x21), 250),
            ],
        );

        assert!(spdd_store.store_spdd(1, spdd_state).is_ok());
        assert!(spdd_store.is_epoch_complete(1).unwrap());

        let result = spdd_store.query_by_epoch(1).unwrap();
        assert_eq!(result.len(), 4);

        let result = spdd_store.query_by_epoch_and_pool(1, &test_pool_hash(0x01)).unwrap();
        assert_eq!(result.len(), 2);
        let result = spdd_store.query_by_epoch_and_pool(1, &test_pool_hash(0x02)).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_retention_pruning() {
        let temp_dir = TempDir::new().unwrap();
        let mut spdd_store =
            SPDDStore::new(temp_dir.path(), 2).expect("Failed to create SPDD store");

        // Store epochs 1, 2, 3
        for epoch in 1..=3 {
            let mut spdd_state: HashMap<PoolId, Vec<(StakeAddress, u64)>> = HashMap::new();
            spdd_state.insert(
                test_pool_hash(epoch as u8),
                vec![
                    (test_stake_address(0x10), epoch * 100),
                    (test_stake_address(0x11), epoch * 150),
                ],
            );
            spdd_store.store_spdd(epoch, spdd_state).expect("Failed to store SPDD state");
        }

        // Epoch 1 should be pruned (retention=2, so keep epochs 2 and 3)
        assert!(!spdd_store.is_epoch_complete(1).unwrap());
        assert!(spdd_store.is_epoch_complete(2).unwrap());
        assert!(spdd_store.is_epoch_complete(3).unwrap());

        assert!(spdd_store.query_by_epoch(1).is_err());
        assert!(spdd_store.query_by_epoch(2).is_ok());
        assert!(spdd_store.query_by_epoch(3).is_ok());
    }

    #[test]
    fn test_query_incomplete_epoch() {
        let temp_dir = TempDir::new().unwrap();
        let spdd_store = SPDDStore::new(temp_dir.path(), 10).expect("Failed to create SPDD store");

        assert!(!spdd_store.is_epoch_complete(999).unwrap());
        assert!(spdd_store.query_by_epoch(999).is_err());
        assert!(spdd_store.query_by_epoch_and_pool(999, &test_pool_hash(0x01)).is_err());
    }

    #[test]
    fn test_remove_epoch_data() {
        let temp_dir = TempDir::new().unwrap();
        let mut spdd_store =
            SPDDStore::new(temp_dir.path(), 10).expect("Failed to create SPDD store");

        let mut spdd_state: HashMap<PoolId, Vec<(StakeAddress, u64)>> = HashMap::new();
        spdd_state.insert(
            test_pool_hash(0x01),
            vec![
                (test_stake_address(0x10), 100),
                (test_stake_address(0x11), 150),
            ],
        );

        spdd_store.store_spdd(1, spdd_state).unwrap();
        assert!(spdd_store.is_epoch_complete(1).unwrap());

        let deleted = spdd_store.remove_epoch_data(1).unwrap();
        assert_eq!(deleted, 2);
        assert!(!spdd_store.is_epoch_complete(1).unwrap());

        let deleted = spdd_store.remove_epoch_data(999).unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let epoch = 12345u64;
        let pool_id = test_pool_hash(0x42);
        let stake_address = test_stake_address(0x99);

        let encoded = encode_key(epoch, &pool_id, &stake_address);
        let (decoded_epoch, decoded_pool, decoded_stake) = decode_key(&encoded).unwrap();

        assert_eq!(decoded_epoch, epoch);
        assert_eq!(decoded_pool, pool_id);
        assert_eq!(decoded_stake, stake_address);
    }
}
