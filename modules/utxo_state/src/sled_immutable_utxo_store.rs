//! On-disk store using Sled for immutable UTXOs

use crate::state::{UTXOKey, UTXOValue, ImmutableUTXOStore};
use async_trait::async_trait;
use sled::Db;
use std::path::Path;
use tokio::task;
use anyhow::Result;

pub struct SledImmutableUTXOStore {
    /// Sled database instance
    db: Db,
}

impl SledImmutableUTXOStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path)?; 
        db.clear()?;
        Ok(Self { db })
    }
}

#[async_trait]
impl ImmutableUTXOStore for SledImmutableUTXOStore {

    /// Add a UTXO
    async fn add_utxo(&self, key: UTXOKey, value: UTXOValue) -> Result<()> {
        let db = self.db.clone();

        // We spawn blocking here to avoid blocking the main executor
        // Real shame sled isn't async natively!
        task::spawn_blocking(move || {
            let key_bytes = key.to_bytes();
            let value_bytes = serde_cbor::to_vec(&value)?;
            db.insert(key_bytes, value_bytes)?;
            Ok(())
        })
        .await?
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTXOKey) -> Result<()> {
        let db = self.db.clone();
        let key_bytes = key.to_bytes();
        task::spawn_blocking(move || {
            db.remove(key_bytes)?;
            Ok(())
        })
        .await?
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTXOKey) -> Result<Option<UTXOValue>> {
        let db = self.db.clone();
        let key_bytes = key.to_bytes();
        task::spawn_blocking(move || {
            let maybe = db.get(key_bytes)?;
            let result = match maybe {
                Some(ivec) => Some(serde_cbor::from_slice(&ivec)?),
                None => None,
            };

            Ok(result)
        })
        .await?
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize> {
        let db = self.db.clone();
        task::spawn_blocking(move || Ok(db.len())).await?
    }
}