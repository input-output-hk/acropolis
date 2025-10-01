use std::collections::HashMap;

use acropolis_common::{Address, AddressTotals, TxIdentifier, UTxOIdentifier};
use anyhow::Result;
use async_trait::async_trait;
use fjall::Partition;

use crate::state::{AddressEntry, AddressStorageConfig};

#[async_trait]
pub trait AddressStore: Send + Sync {
    async fn get_utxos(&self, address: &Address) -> Result<Option<Vec<UTxOIdentifier>>>;
    async fn get_txs(&self, address: &Address) -> Result<Option<Vec<TxIdentifier>>>;
    async fn get_totals(&self, address: &Address) -> Result<Option<AddressTotals>>;

    async fn persist_epoch(
        &self,
        epoch: u64,
        drained_blocks: Vec<HashMap<Address, AddressEntry>>,
        config: &AddressStorageConfig,
    ) -> Result<()>;

    async fn get_last_epoch_stored(&self) -> Result<Option<u64>>;
    async fn epoch_exists(
        &self,
        partition: Partition,
        key: &'static [u8],
        epoch: u64,
    ) -> Result<bool>;
}
