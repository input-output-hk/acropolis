//! In-memory store for immutable UTXOs using standard HashMap

use crate::state::{UTXOKey, UTXOValue, ImmutableUTXOStore};
use std::collections::HashMap;
use async_trait::async_trait;
use tokio::sync::RwLock;
use anyhow::Result;
use config::Config;
use std::sync::Arc;
use tracing::info;

pub struct InMemoryImmutableUTXOStore {
    /// Map of UTXOs
    utxos: RwLock<HashMap<UTXOKey, UTXOValue>>,
}

impl InMemoryImmutableUTXOStore {
    pub fn new(_config: Arc<Config>) -> Self {
        info!("Storing immutable UTXOs in memory (standard)");
 
        Self {
            utxos: RwLock::new(HashMap::new())
        }
    }
}

#[async_trait]
impl ImmutableUTXOStore for InMemoryImmutableUTXOStore {

    /// Add a UTXO
    async fn add_utxo(&self, key: UTXOKey, value: UTXOValue) -> Result<()> {
        self.utxos.write().await.insert(key, value);
        Ok(())
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTXOKey) -> Result<()> {
        self.utxos.write().await.remove(key);
        Ok(())
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTXOKey) -> Result<Option<UTXOValue>> {
        // Essential to clone here because ref is not async safe
        Ok(self.utxos.read().await.get(key).cloned())
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize> {
        Ok(self.utxos.read().await.len())
    }
}