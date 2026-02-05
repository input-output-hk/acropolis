//! In-memory store for immutable UTXOs using standard HashMap

use crate::state::ImmutableUTXOStore;
use acropolis_common::{ShelleyAddressPointer, UTXOValue, UTxOIdentifier};
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

    /// Cancel all unspent Byron redeem (AVVM) addresses.
    /// Returns the list of cancelled UTxOs (identifier and value).
    async fn cancel_redeem_utxos(&self) -> Result<Vec<(UTxOIdentifier, UTXOValue)>> {
        let mut utxos = self.utxos.write().await;
        let mut cancelled = Vec::new();

        // Find all redeem addresses
        let keys_to_remove: Vec<_> = utxos
            .iter()
            .filter(|(_, utxo)| utxo.address.is_redeem())
            .map(|(key, _)| *key)
            .collect();

        // Remove them and collect the cancelled UTxOs
        for key in keys_to_remove {
            if let Some(utxo) = utxos.remove(&key) {
                cancelled.push((key, utxo));
            }
        }

        let total_cancelled: u64 = cancelled.iter().map(|(_, u)| u.value.lovelace).sum();
        info!(
            count = cancelled.len(),
            total_cancelled, "Cancelled AVVM/redeem UTxOs"
        );

        Ok(cancelled)
    }

    /// Get the total lovelace of UTXOs in the store
    async fn sum_lovelace(&self) -> Result<u64> {
        Ok(self.utxos.read().await.values().map(|v| v.value.lovelace).sum())
    }

    /// Sum all unspent UTxOs at pointer addresses, grouped by pointer.
    async fn sum_pointer_utxos(&self) -> Result<HashMap<ShelleyAddressPointer, u64>> {
        let utxos = self.utxos.read().await;
        let mut result: HashMap<ShelleyAddressPointer, u64> = HashMap::new();

        for utxo in utxos.values() {
            if let Some(ptr) = utxo.address.get_pointer() {
                *result.entry(ptr).or_insert(0) += utxo.value.lovelace;
            }
        }

        Ok(result)
    }
}
