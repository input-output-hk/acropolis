//! In-memory store for immutable UTXOs

use crate::state::{UTXOKey, UTXOValue, ImmutableUTXOStore};
use std::collections::HashMap;
use tokio::sync::Mutex;
use async_trait::async_trait;

pub struct InMemoryImmutableUTXOStore {
    /// Map of UTXOs
    utxos: Mutex<HashMap<UTXOKey, UTXOValue>>,
}

impl InMemoryImmutableUTXOStore {
    pub fn new() -> Self { 
        Self {
            utxos: Mutex::new(HashMap::new())
        }
    }
}

#[async_trait]
impl ImmutableUTXOStore for InMemoryImmutableUTXOStore {

    /// Add a UTXO
    async fn add_utxo(&self, key: UTXOKey, value: UTXOValue) {
        self.utxos.lock().await.insert(key, value);
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTXOKey) {
        self.utxos.lock().await.remove(key);
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTXOKey) -> Option<UTXOValue> {
        return self.utxos.lock().await.get(key).cloned();
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> usize {
        return self.utxos.lock().await.len();
    }
}