//! On-disk store using Fjall for immutable UTXOs

use crate::state::{ImmutableUTXOStore, UTXOKey, UTXOValue};
use async_trait::async_trait;
use fjall::{Config, Keyspace, Partition, PartitionCreateOptions, PersistMode};
use std::path::Path;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use anyhow::Result;

pub struct FjallImmutableUTXOStore {
    keyspace: Keyspace,
    partition: Partition,
    write_counter: AtomicUsize,
    flush_every: AtomicUsize,
}

const DEFAULT_FLUSH_EVERY: usize = 1000;
const PARTITION_NAME: &str = "utxos";

impl FjallImmutableUTXOStore {
    /// Create a new Fjall-backed UTXO store with default flush threshold (1000)
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {

        let path = path.as_ref();

        // Clear down before start
        if path.exists() {
            fs::remove_dir_all(path)?;
        }
    
        let keyspace = Config::new(path).open()?;
        let partition = keyspace.open_partition(PARTITION_NAME,
            PartitionCreateOptions::default())?;

        Ok(Self {
            keyspace,
            partition,
            write_counter: AtomicUsize::new(0),
            flush_every: AtomicUsize::new(DEFAULT_FLUSH_EVERY),
        })
    }

    /// Set the flush frequency (number of writes before flushing to disk)
    pub fn set_flush_every(&self, n: usize) {
        self.flush_every.store(n, Ordering::Relaxed);
    }

    /// Check if a flush is needed
    fn should_flush(&self) -> bool {
        let count = self.write_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let threshold = self.flush_every.load(Ordering::Relaxed);
        threshold != 0 && count % threshold == 0
    }
}

#[async_trait]
impl ImmutableUTXOStore for FjallImmutableUTXOStore {
    async fn add_utxo(&self, key: UTXOKey, value: UTXOValue) -> Result<()> {
        let key_bytes = key.to_bytes();
        let value_bytes = serde_cbor::to_vec(&value)?;
        let should_flush = self.should_flush();

        self.partition.insert(key_bytes, value_bytes)?;
        if should_flush {
            self.keyspace.persist(PersistMode::Buffer)?;
        }

        Ok(())
    }

    async fn delete_utxo(&self, key: &UTXOKey) -> Result<()> {
                let key_bytes = key.to_bytes();
        let should_flush = self.should_flush();

        self.partition.remove(key_bytes)?;
        if should_flush {
            self.keyspace.persist(PersistMode::Buffer)?;
        }
        Ok(())
    }

    async fn lookup_utxo(&self, key: &UTXOKey) -> Result<Option<UTXOValue>> {
        let key_bytes = key.to_bytes();
        Ok(match self.partition.get(key_bytes)? {
            Some(ivec) => Some(serde_cbor::from_slice(&ivec)?),
            None => None,
        })
    }

    async fn len(&self) -> Result<usize> {
        Ok(self.partition.len()?)
    }
}
