//! On-disk store using Fjall for immutable UTXOs

use crate::state::ImmutableUTXOStore;
use acropolis_common::{ShelleyAddressPointer, UTXOValue, UTxOIdentifier};
use anyhow::Result;
use async_trait::async_trait;
use config::Config;
use fjall::{Database, Keyspace, KeyspaceCreateOptions, PersistMode};
use std::collections::HashMap;
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
const DEFAULT_NETWORK_NAME: &str = "mainnet";

impl FjallImmutableUTXOStore {
    /// Create a new Fjall-backed UTXO store with default flush threshold (1000)
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let path = config.get_string("database-path").unwrap_or_else(|_| {
            format!(
                "{DEFAULT_DATABASE_PATH}-{}",
                Self::network_scope_from_config(config.as_ref())
            )
        });
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

    fn network_scope_from_config(config: &Config) -> String {
        config
            .get_string("startup.network-name")
            .or_else(|_| config.get_string("network-name"))
            .or_else(|_| config.get_string("network-id"))
            .unwrap_or_else(|_| DEFAULT_NETWORK_NAME.to_string())
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

    /// Cancel all unspent Byron redeem (AVVM) addresses.
    /// Returns the list of cancelled UTxOs (identifier and value).
    async fn cancel_redeem_utxos(&self) -> Result<Vec<(UTxOIdentifier, UTXOValue)>> {
        let mut cancelled = Vec::new();

        // Iterate over all UTxOs and collect redeem addresses
        for entry in self.keyspace.iter() {
            let (key_bytes, value_bytes) = entry.into_inner()?;
            let utxo: UTXOValue = serde_cbor::from_slice(&value_bytes)?;
            if utxo.address.is_redeem() {
                let key = UTxOIdentifier::from_bytes(&key_bytes)?;
                cancelled.push((key, utxo));
            }
        }

        // Remove them
        for (key, _) in &cancelled {
            self.keyspace.remove(key.to_bytes())?;
        }

        // Flush after mass delete
        self.database.persist(PersistMode::Buffer)?;

        let total_cancelled: u64 = cancelled.iter().map(|(_, u)| u.value.lovelace).sum();
        info!(
            count = cancelled.len(),
            total_cancelled, "Cancelled AVVM/redeem UTxOs"
        );

        Ok(cancelled)
    }

    /// Get the total lovelace of UTXOs in the store
    async fn sum_lovelace(&self) -> Result<u64> {
        self.keyspace.iter().try_fold(0u64, |acc, item| {
            let bytes = item.value()?;
            if let Ok(utxo) = serde_cbor::from_slice::<UTXOValue>(&bytes) {
                Ok(acc + utxo.value.lovelace)
            } else {
                Ok(acc)
            }
        })
    }

    async fn sum_pointer_utxos(&self) -> Result<HashMap<ShelleyAddressPointer, u64>> {
        let mut result: HashMap<ShelleyAddressPointer, u64> = HashMap::new();

        for entry in self.keyspace.iter() {
            let value_bytes = entry.value()?;
            let utxo: UTXOValue = serde_cbor::from_slice(&value_bytes)?;
            if let Some(ptr) = utxo.address.get_pointer() {
                *result.entry(ptr).or_insert(0) += utxo.value.lovelace;
            }
        }

        Ok(result)
    }
}
