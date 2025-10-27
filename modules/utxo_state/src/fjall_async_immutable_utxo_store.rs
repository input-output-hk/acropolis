//! On-disk store using Fjall for immutable UTXOs

use crate::state::{ImmutableUTXOStore, UTXOValue};
use acropolis_common::UTxOIdentifier;
use anyhow::Result;
use async_trait::async_trait;
use config::Config;
use fjall::{Keyspace, Partition, PartitionCreateOptions, PersistMode};
use std::fs;
use std::path::Path;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::task;
use tracing::info;

pub struct FjallAsyncImmutableUTXOStore {
    keyspace: Keyspace,
    partition: Partition,
    write_counter: AtomicUsize,
    flush_every: AtomicUsize,
}

const DEFAULT_FLUSH_EVERY: i64 = 1000;
const DEFAULT_DATABASE_PATH: &str = "fjall-immutable-utxos";
const PARTITION_NAME: &str = "utxos";

impl FjallAsyncImmutableUTXOStore {
    /// Create a new Fjall-backed UTXO store with default flush threshold (1000)
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let path = config.get_string("database-path").unwrap_or(DEFAULT_DATABASE_PATH.to_string());
        info!("Storing immutable UTXOs with Fjall (async) on disk ({path})");

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
impl ImmutableUTXOStore for FjallAsyncImmutableUTXOStore {
    async fn add_utxo(&self, key: UTxOIdentifier, value: UTXOValue) -> Result<()> {
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

    async fn delete_utxo(&self, key: &UTxOIdentifier) -> Result<()> {
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

    async fn lookup_utxo(&self, key: &UTxOIdentifier) -> Result<Option<UTXOValue>> {
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
        Ok(
            task::spawn_blocking(move || Ok::<_, anyhow::Error>(partition.approximate_len()))
                .await??,
        )
    }
}
