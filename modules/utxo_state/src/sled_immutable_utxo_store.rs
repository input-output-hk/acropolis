//! On-disk store using Sled for immutable UTXOs

use crate::state::{UTXOKey, UTXOValue, ImmutableUTXOStore};
use async_trait::async_trait;
use sled::Db;
use std::path::Path;
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
        let maybe = self.db.get(key_bytes)?;
        let result = match maybe {
            Some(ivec) => Some(serde_cbor::from_slice(&ivec)?),
            None => None,
        };
        Ok(result)
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize> {
        Ok(self.db.len())
    }
}