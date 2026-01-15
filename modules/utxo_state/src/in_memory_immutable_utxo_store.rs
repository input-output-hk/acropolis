//! In-memory store for immutable UTXOs using standard HashMap

use crate::state::ImmutableUTXOStore;
use acropolis_common::{UTXOValue, UTxOIdentifier, Value};
use anyhow::Result;
use async_trait::async_trait;
use config::Config;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

pub struct InMemoryImmutableUTXOStore {
    /// Map of UTXOs
    utxos: RwLock<HashMap<UTxOIdentifier, UTXOValue>>,
}

impl InMemoryImmutableUTXOStore {
    pub fn new(_config: Arc<Config>) -> Self {
        info!("Storing immutable UTXOs in memory (standard)");

        Self {
            utxos: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ImmutableUTXOStore for InMemoryImmutableUTXOStore {
    /// Add a UTXO
    async fn add_utxo(&self, key: UTxOIdentifier, value: UTXOValue) -> Result<()> {
        self.utxos.write().await.insert(key, value);
        Ok(())
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTxOIdentifier) -> Result<()> {
        self.utxos.write().await.remove(key);
        Ok(())
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTxOIdentifier) -> Result<Option<UTXOValue>> {
        // Essential to clone here because ref is not async safe
        Ok(self.utxos.read().await.get(key).cloned())
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize> {
        Ok(self.utxos.read().await.len())
    }

    /// Get the total value of UTXOs in the store
    async fn sum(&self) -> Result<Value> {
        Ok(self.utxos.read().await.values().map(|v| &v.value).sum())
    }
}
