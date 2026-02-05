//! In-memory store for immutable UTXOs using DashMap
// Faster and API is simpler because it uses internally sharded locks
// but it takes a lot more memory than HashMap

use crate::state::ImmutableUTXOStore;
use acropolis_common::{ShelleyAddressPointer, UTXOValue, UTxOIdentifier};
use anyhow::Result;
use async_trait::async_trait;
use config::Config;
use dashmap::DashMap;
use std::collections::HashMap;
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

    /// Cancel all unspent Byron redeem (AVVM) addresses.
    /// Returns the list of cancelled UTxOs (identifier and value).
    async fn cancel_redeem_utxos(&self) -> Result<Vec<(UTxOIdentifier, UTXOValue)>> {
        let mut cancelled = Vec::new();

        // Find all redeem addresses
        let keys_to_remove: Vec<_> = self
            .utxos
            .iter()
            .filter(|entry| entry.value().address.is_redeem())
            .map(|entry| *entry.key())
            .collect();

        // Remove them and collect the cancelled UTxOs
        for key in keys_to_remove {
            if let Some((key, utxo)) = self.utxos.remove(&key) {
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
        Ok(self.utxos.iter().map(|entry| entry.value().value.lovelace).sum())
    }

    async fn sum_pointer_utxos(&self) -> Result<HashMap<ShelleyAddressPointer, u64>> {
        let mut result: HashMap<ShelleyAddressPointer, u64> = HashMap::new();

        for entry in self.utxos.iter() {
            if let Some(ptr) = entry.value().address.get_pointer() {
                *result.entry(ptr).or_insert(0) += entry.value().value.lovelace;
            }
        }

        Ok(result)
    }
}
