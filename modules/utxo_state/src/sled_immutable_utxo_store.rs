//! On-disk store using Sled for immutable UTXOs

use crate::state::{ImmutableUTXOStore, UTXOKey, UTXOValue};
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
    async fn add_utxo(&self, key: UTXOKey, value: UTXOValue) -> Result<()> {
        let key_bytes = key.to_bytes();
        let value_bytes = serde_cbor::to_vec(&value)?;
        self.db.insert(key_bytes, value_bytes)?;
        Ok(())
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTXOKey) -> Result<()> {
        let key_bytes = key.to_bytes();
        self.db.remove(key_bytes)?;
        Ok(())
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTXOKey) -> Result<Option<UTXOValue>> {
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
}
