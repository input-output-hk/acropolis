//! Fake store for immutable UTXOs

use crate::state::ImmutableUTXOStore;
use acropolis_common::{Address, ShelleyAddressPointer, UTXOValue, UTxOIdentifier, Value};
use anyhow::Result;
use async_trait::async_trait;
use config::Config;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info};

pub struct FakeImmutableUTXOStore {
    /// Delay time for testing
    delay_us: u64,
}

impl FakeImmutableUTXOStore {
    pub fn new(config: Arc<Config>) -> Self {
        error!("Using fake immutable UTXO store!");

        let delay_us = config.get_int("delay").unwrap_or(0) as u64;
        if delay_us != 0 {
            info!("Delay of {delay_us}us on each write");
        }

        Self { delay_us }
    }
}

#[async_trait]
impl ImmutableUTXOStore for FakeImmutableUTXOStore {
    /// Add a UTXO
    async fn add_utxo(&self, _key: UTxOIdentifier, _value: UTXOValue) -> Result<()> {
        if self.delay_us != 0 {
            sleep(Duration::from_micros(self.delay_us)).await;
        }

        Ok(())
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, _key: &UTxOIdentifier) -> Result<()> {
        if self.delay_us != 0 {
            sleep(Duration::from_micros(self.delay_us)).await;
        }

        Ok(())
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, _key: &UTxOIdentifier) -> Result<Option<UTXOValue>> {
        Ok(Some(UTXOValue {
            address: Address::None,
            value: Value::new(42, Vec::new()),
            datum: None,
            reference_script_hash: None,
        }))
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize> {
        Ok(42)
    }

    /// Cancel all unspent Byron redeem (AVVM) addresses.
    /// Returns the list of cancelled UTxOs (identifier and value).
    async fn cancel_redeem_utxos(&self) -> Result<Vec<(UTxOIdentifier, UTXOValue)>> {
        // Fake store doesn't track actual UTxOs
        Ok(Vec::new())
    }

    /// Get the total lovelace of UTXOs in the store
    async fn sum_lovelace(&self) -> Result<u64> {
        Ok(0)
    }

    /// Sum all unspent UTxOs at pointer addresses, grouped by pointer.
    async fn sum_pointer_utxos(&self) -> Result<HashMap<ShelleyAddressPointer, u64>> {
        // Fake store doesn't track actual UTxOs
        Ok(HashMap::new())
    }
}
