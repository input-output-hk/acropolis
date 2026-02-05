//! On-disk store using Fjall for immutable UTXOs

use crate::state::ImmutableUTXOStore;
use acropolis_common::{ShelleyAddressPointer, UTXOValue, UTxOIdentifier};
use anyhow::Result;
use async_trait::async_trait;
use config::Config;
use fjall::{Keyspace, Partition, PartitionCreateOptions, PersistMode};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tracing::info;

pub struct FjallImmutableUTXOStore {
    keyspace: Keyspace,
    partition: Partition,
    write_counter: AtomicUsize,
    flush_every: AtomicUsize,
}

const DEFAULT_FLUSH_EVERY: i64 = 1000;
const DEFAULT_DATABASE_PATH: &str = "fjall-immutable-utxos";
const PARTITION_NAME: &str = "utxos";

impl FjallImmutableUTXOStore {
    /// Create a new Fjall-backed UTXO store with default flush threshold (1000)
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let path = config.get_string("database-path").unwrap_or(DEFAULT_DATABASE_PATH.to_string());
        info!("Storing immutable UTXOs with Fjall (sync) on disk ({path})");
        let path = Path::new(&path);

        // Clear down before start
        if path.exists() {
            fs::remove_dir_all(path)?;
        }

        let mut fjall_config = fjall::Config::new(path);
        fjall_config = fjall_config.manual_journal_persist(true); // We're in control of flushing
        let keyspace = fjall_config.open()?;
        let partition =
            keyspace.open_partition(PARTITION_NAME, PartitionCreateOptions::default())?;

        let flush_every = config.get_int("flush-every").unwrap_or(DEFAULT_FLUSH_EVERY);

        Ok(Self {
            keyspace,
            partition,
            write_counter: AtomicUsize::new(0),
            flush_every: AtomicUsize::new(flush_every as usize),
        })
    }

    /// Check if a flush is needed
    fn should_flush(&self) -> bool {
        let count = self.write_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let threshold = self.flush_every.load(Ordering::Relaxed);
        threshold != 0 && count.is_multiple_of(threshold)
    }
}

#[async_trait]
impl ImmutableUTXOStore for FjallImmutableUTXOStore {
    async fn add_utxo(&self, key: UTxOIdentifier, value: UTXOValue) -> Result<()> {
        let key_bytes = key.to_bytes();
        let value_bytes = serde_cbor::to_vec(&value)?;
        let should_flush = self.should_flush();

        self.partition.insert(key_bytes, value_bytes)?;
        if should_flush {
            self.keyspace.persist(PersistMode::Buffer)?;
        }

        Ok(())
    }

    async fn delete_utxo(&self, key: &UTxOIdentifier) -> Result<()> {
        let key_bytes = key.to_bytes();
        let should_flush = self.should_flush();

        self.partition.remove(key_bytes)?;
        if should_flush {
            self.keyspace.persist(PersistMode::Buffer)?;
        }
        Ok(())
    }

    async fn lookup_utxo(&self, key: &UTxOIdentifier) -> Result<Option<UTXOValue>> {
        let key_bytes = key.to_bytes();
        Ok(match self.partition.get(key_bytes)? {
            Some(ivec) => Some(serde_cbor::from_slice(&ivec)?),
            None => None,
        })
    }

    async fn len(&self) -> Result<usize> {
        Ok(self.partition.approximate_len())
    }

    /// Cancel all unspent Byron redeem (AVVM) addresses.
    /// Returns the list of cancelled UTxOs (identifier and value).
    async fn cancel_redeem_utxos(&self) -> Result<Vec<(UTxOIdentifier, UTXOValue)>> {
        let mut cancelled = Vec::new();

        // Iterate over all UTxOs and collect redeem addresses
        for entry in self.partition.iter() {
            let (key_bytes, value_bytes) = entry?;
            let utxo: UTXOValue = serde_cbor::from_slice(&value_bytes)?;
            if utxo.address.is_redeem() {
                let key = UTxOIdentifier::from_bytes(&key_bytes)?;
                cancelled.push((key, utxo));
            }
        }

        // Remove them
        for (key, _) in &cancelled {
            self.partition.remove(key.to_bytes())?;
        }

        // Flush after mass delete
        self.keyspace.persist(PersistMode::Buffer)?;

        let total_cancelled: u64 = cancelled.iter().map(|(_, u)| u.value.lovelace).sum();
        info!(
            count = cancelled.len(),
            total_cancelled, "Cancelled AVVM/redeem UTxOs"
        );

        Ok(cancelled)
    }

    /// Get the total lovelace of UTXOs in the store
    async fn sum_lovelace(&self) -> Result<u64> {
        self.partition.iter().try_fold(0u64, |acc, item| {
            let (_k, bytes) = item?;
            if let Ok(utxo) = serde_cbor::from_slice::<UTXOValue>(&bytes) {
                Ok(acc + utxo.value.lovelace)
            } else {
                Ok(acc)
            }
        })
    }

    /// Sum all unspent UTxOs at pointer addresses, grouped by pointer.
    async fn sum_pointer_utxos(&self) -> Result<HashMap<ShelleyAddressPointer, u64>> {
        let mut result: HashMap<ShelleyAddressPointer, u64> = HashMap::new();

        for entry in self.partition.iter() {
            let (_key_bytes, value_bytes) = entry?;
            let utxo: UTXOValue = serde_cbor::from_slice(&value_bytes)?;
            if let Some(ptr) = utxo.address.get_pointer() {
                *result.entry(ptr).or_insert(0) += utxo.value.lovelace;
            }
        }

        Ok(result)
    }
}
