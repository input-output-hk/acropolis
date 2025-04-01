//! Fake store for immutable UTXOs

use crate::state::{UTXOKey, UTXOValue, ImmutableUTXOStore};
use acropolis_common::Address;
use async_trait::async_trait;
use anyhow::Result;
use config::Config;
use std::sync::Arc;
use tracing::{error, info};
use tokio::time::{sleep, Duration};

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

        Self {
            delay_us,
        }
    }
}

#[async_trait]
impl ImmutableUTXOStore for FakeImmutableUTXOStore {

    /// Add a UTXO
    async fn add_utxo(&self, _key: UTXOKey, _value: UTXOValue) -> Result<()> {
        if self.delay_us != 0 {
            sleep(Duration::from_micros(self.delay_us)).await;
        }

        Ok(())
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, _key: &UTXOKey) -> Result<()> {
        if self.delay_us != 0 {
            sleep(Duration::from_micros(self.delay_us)).await;
        }

        Ok(())
    }

    /// Lookup a UTXO
    async fn lookup_utxo(&self, _key: &UTXOKey) -> Result<Option<UTXOValue>> {
        Ok(Some(UTXOValue{
            address: Address::None,
            value: 42
        }))
    }

    /// Get the number of UTXOs in the store
    async fn len(&self) -> Result<usize> {
        Ok(42)
    }
}