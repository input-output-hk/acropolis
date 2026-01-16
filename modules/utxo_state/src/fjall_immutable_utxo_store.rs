//! On-disk store using Fjall for immutable UTXOs

use crate::state::ImmutableUTXOStore;
use acropolis_common::{UTXOValue, UTxOIdentifier};
use anyhow::Result;
use async_trait::async_trait;
use config::Config;
use fjall::{Database, Keyspace, KeyspaceCreateOptions, PersistMode};
use std::fs;
use std::path::Path;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tracing::info;

pub struct FjallImmutableUTXOStore {
    database: Database,
    keyspace: Keyspace,
    write_counter: AtomicUsize,
    flush_every: AtomicUsize,
}

const DEFAULT_FLUSH_EVERY: i64 = 1000;
const DEFAULT_DATABASE_PATH: &str = "fjall-immutable-utxos";
const KEYSPACE_NAME: &str = "utxos";

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

        let database = Database::builder(path).manual_journal_persist(true).open()?;
        let keyspace = database.keyspace(KEYSPACE_NAME, KeyspaceCreateOptions::default)?;

        let flush_every = config.get_int("flush-every").unwrap_or(DEFAULT_FLUSH_EVERY);

        Ok(Self {
            database,
            keyspace,
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

        self.keyspace.insert(key_bytes, value_bytes)?;
        if should_flush {
            self.database.persist(PersistMode::Buffer)?;
        }

        Ok(())
    }

    async fn delete_utxo(&self, key: &UTxOIdentifier) -> Result<()> {
        let key_bytes = key.to_bytes();
        let should_flush = self.should_flush();

        self.keyspace.remove(key_bytes)?;
        if should_flush {
            self.database.persist(PersistMode::Buffer)?;
        }
        Ok(())
    }

    async fn lookup_utxo(&self, key: &UTxOIdentifier) -> Result<Option<UTXOValue>> {
        let key_bytes = key.to_bytes();
        Ok(match self.keyspace.get(key_bytes)? {
            Some(ivec) => Some(serde_cbor::from_slice(&ivec)?),
            None => None,
        })
    }

    async fn len(&self) -> Result<usize> {
        Ok(self.keyspace.approximate_len())
    }
}
