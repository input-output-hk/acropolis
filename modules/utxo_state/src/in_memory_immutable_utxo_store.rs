//! In-memory store for immutable UTXOs

use crate::state::{UTXOKey, UTXOValue, ImmutableUTXOStore};
use dashmap::DashMap;
use async_trait::async_trait;

pub struct InMemoryImmutableUTXOStore {
    /// Map of UTXOs
    utxos: DashMap<UTXOKey, UTXOValue>,
}

impl InMemoryImmutableUTXOStore {
    pub fn new() -> Self { 
        Self {
            utxos: DashMap::new()
        }
    }
}

#[async_trait]
impl ImmutableUTXOStore for InMemoryImmutableUTXOStore {

    /// Add a UTXO
    async fn add_utxo(&self, key: UTXOKey, value: UTXOValue) {
        self.utxos.insert(key, value);
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTXOKey) {
        self.utxos.remove(key);
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTXOKey) -> Option<UTXOValue> {
        // Essential to clone here because ref is not async safe
        return self.utxos.get(key).map(|value| value.clone());
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> usize {
        return self.utxos.len();
    }
}