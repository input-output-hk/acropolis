//! On-disk store using Sled for immutable UTXOs

use crate::state::ImmutableUTXOStore;
use acropolis_common::{UTXOValue, UTxOIdentifier};
use anyhow::Result;
use async_trait::async_trait;
use config::Config;
use sled::Db;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

const DEFAULT_DATABASE_PATH: &str = "sled-immutable-utxos";

pub struct SledImmutableUTXOStore {
    /// Sled database instance
    db: Db,
}

impl SledImmutableUTXOStore {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let path = config.get_string("database-path").unwrap_or(DEFAULT_DATABASE_PATH.to_string());
        info!("Storing immutable UTXOs with Sled (sync) on disk ({path})");

        let path = Path::new(&path);

        // Clear down before start
        if path.exists() {
            fs::remove_dir_all(path)?;
        }

        let db = sled::open(path)?;
        Ok(Self { db })
    }
}

#[async_trait]
impl ImmutableUTXOStore for SledImmutableUTXOStore {
    /// Add a UTXO
    async fn add_utxo(&self, key: UTxOIdentifier, value: UTXOValue) -> Result<()> {
        let key_bytes = key.to_bytes();
        let value_bytes = serde_cbor::to_vec(&value)?;
        self.db.insert(key_bytes, value_bytes)?;
        Ok(())
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTxOIdentifier) -> Result<()> {
        let key_bytes = key.to_bytes();
        self.db.remove(key_bytes)?;
        Ok(())
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTxOIdentifier) -> Result<Option<UTXOValue>> {
        let key_bytes = key.to_bytes();
        Ok(match self.db.get(key_bytes)? {
            Some(ivec) => Some(serde_cbor::from_slice(&ivec)?),
            None => None,
        })
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize> {
        Ok(self.db.len())
    }

    /// Cancel all unspent Byron redeem (AVVM) addresses.
    /// Returns the list of cancelled UTxOs (identifier and value).
    async fn cancel_redeem_utxos(&self) -> Result<Vec<(UTxOIdentifier, UTXOValue)>> {
        let mut cancelled = Vec::new();

        // Iterate over all UTxOs and collect redeem addresses
        for entry in self.db.iter() {
            let (key_bytes, value_bytes) = entry?;
            let utxo: UTXOValue = serde_cbor::from_slice(&value_bytes)?;
            if utxo.address.is_redeem() {
                let key = UTxOIdentifier::from_bytes(&key_bytes)?;
                cancelled.push((key, utxo));
            }
        }

        // Remove them
        for (key, _) in &cancelled {
            self.db.remove(key.to_bytes())?;
        }

        // Flush
        self.db.flush()?;

        let total_cancelled: u64 = cancelled.iter().map(|(_, u)| u.value.lovelace).sum();
        info!(
            count = cancelled.len(),
            total_cancelled, "Cancelled AVVM/redeem UTxOs"
        );

        Ok(cancelled)
    }

    /// Get the total lovelace of UTXOs in the store
    async fn sum_lovelace(&self) -> Result<u64> {
        self.db.iter().try_fold(0u64, |acc, item| {
            let (_k, bytes) = item?;
            if let Ok(utxo) = serde_cbor::from_slice::<UTXOValue>(&bytes) {
                Ok(acc + utxo.value.lovelace)
            } else {
                Ok(acc)
            }
        })
    }
}
