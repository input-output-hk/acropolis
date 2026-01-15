//! In-memory store for immutable UTXOs using DashMap
// Faster and API is simpler because it uses internally sharded locks
// but it takes a lot more memory than HashMap

use crate::state::ImmutableUTXOStore;
use acropolis_common::{UTXOValue, UTxOIdentifier, Value};
use anyhow::Result;
use async_trait::async_trait;
use config::Config;
use dashmap::DashMap;
use std::sync::Arc;
use tracing::info;

pub struct DashMapImmutableUTXOStore {
    /// Map of UTXOs
    utxos: DashMap<UTxOIdentifier, UTXOValue>,
}

impl DashMapImmutableUTXOStore {
    pub fn new(_config: Arc<Config>) -> Self {
        info!("Storing immutable UTXOs in memory (DashMap)");
        Self {
            utxos: DashMap::new(),
        }
    }
}

#[async_trait]
impl ImmutableUTXOStore for DashMapImmutableUTXOStore {
    /// Add a UTXO
    async fn add_utxo(&self, key: UTxOIdentifier, value: UTXOValue) -> Result<()> {
        self.utxos.insert(key, value);
        Ok(())
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, key: &UTxOIdentifier) -> Result<()> {
        self.utxos.remove(key);
        Ok(())
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, key: &UTxOIdentifier) -> Result<Option<UTXOValue>> {
        // Essential to clone here because ref is not async safe
        Ok(self.utxos.get(key).map(|value| value.clone()))
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize> {
        Ok(self.utxos.len())
    }

    /// Get the total value of UTXOs in the store
    async fn sum(&self) -> Result<Value> {
        Ok(self.utxos.iter().fold(Value::default(), |mut acc, entry| {
            acc += &entry.value().value;
            acc
        }))
    }
}
