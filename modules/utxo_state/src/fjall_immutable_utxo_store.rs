//! On-disk store using Fjall for immutable UTXOs

use crate::state::{ImmutableUTXOStore, UTXOKey, UTXOValue};
use async_trait::async_trait;
use fjall::{Config, Keyspace, Partition, PartitionCreateOptions, PersistMode};
use std::path::Path;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::task;
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
        let partition = self.partition.clone();
        let keyspace = self.keyspace.clone();
        let key_bytes = key.to_bytes();
        let value_bytes = serde_cbor::to_vec(&value)?;
        let should_flush = self.should_flush();

        task::spawn_blocking(move || {
            partition.insert(key_bytes, value_bytes)?;
            if should_flush {
                keyspace.persist(PersistMode::Buffer)?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;

        Ok(())
    }

    async fn delete_utxo(&self, key: &UTXOKey) -> Result<()> {
        let partition = self.partition.clone();
        let keyspace = self.keyspace.clone();
        let key_bytes = key.to_bytes();
        let should_flush = self.should_flush();

        task::spawn_blocking(move || {
            partition.remove(key_bytes)?;
            if should_flush {
                keyspace.persist(PersistMode::Buffer)?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;

        Ok(())
    }

    async fn lookup_utxo(&self, key: &UTXOKey) -> Result<Option<UTXOValue>> {
        let partition = self.partition.clone();
        let key_bytes = key.to_bytes();

        Ok(task::spawn_blocking(move || {
            let maybe = partition.get(key_bytes)?;
            let result = match maybe {
                Some(ivec) => Some(serde_cbor::from_slice(&ivec)?),
                None => None,
            };
            Ok::<_, anyhow::Error>(result)
        })
        .await??)
    }

    async fn len(&self) -> Result<usize> {
        let partition = self.partition.clone();
        Ok(task::spawn_blocking(move || {
            Ok::<_, anyhow::Error>(partition.len()?)
        })
        .await??)
    }
}
