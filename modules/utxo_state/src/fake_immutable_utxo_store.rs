//! Fake store for immutable UTXOs

use crate::state::{UTXOKey, UTXOValue, ImmutableUTXOStore};
use acropolis_common::Address;
use async_trait::async_trait;
use anyhow::Result;
use config::Config;
use std::sync::Arc;
use tracing::error;

pub struct FakeImmutableUTXOStore {
}

impl FakeImmutableUTXOStore {
    pub fn new(_config: Arc<Config>) -> Self { 
        error!("Using fake immutable UTXO store!");
        Self { }
    }
}

#[async_trait]
impl ImmutableUTXOStore for FakeImmutableUTXOStore {

    /// Add a UTXO
    async fn add_utxo(&self, _key: UTXOKey, _value: UTXOValue) -> Result<()> {
        Ok(())
    }

    /// Delete a UTXO
    async fn delete_utxo(&self, _key: &UTXOKey) -> Result<()> {
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