//! In-memory store for immutable UTXOs using DashMap
// Faster and API is simpler because it uses internally sharded locks
// but it takes a lot more memory than HashMap

use crate::state::{UTXOKey, UTXOValue, ImmutableUTXOStore};
use dashmap::DashMap;
use async_trait::async_trait;
use anyhow::Result;
use config::Config;
use std::sync::Arc;
use tracing::info;

pub struct DashMapImmutableUTXOStore {
    /// Map of UTXOs
    utxos: DashMap<UTXOKey, UTXOValue>,
}

impl DashMapImmutableUTXOStore {
    pub fn new(_config: Arc<Config>) -> Self { 
        info!("Storing immutable UTXOs in memory (DashMap)");
        Self {
            utxos: DashMap::new()
        }
    }
}

#[async_trait]
impl ImmutableUTXOStore for DashMapImmutableUTXOStore {

    /// Add a UTXO
    async fn add_utxo(&self, key: UTXOKey, value: UTXOValue) -> Result<()> {
        self.utxos.insert(key, value);
        Ok(())
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTXOKey) -> Result<()> {
        self.utxos.remove(key);
        Ok(())
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTXOKey) -> Result<Option<UTXOValue>> {
        // Essential to clone here because ref is not async safe
        Ok(self.utxos.get(key).map(|value| value.clone()))
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize> {
        Ok(self.utxos.len())
    }
}