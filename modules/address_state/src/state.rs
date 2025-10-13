use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use acropolis_common::{
    Address, AddressDelta, AddressTotals, BlockInfo, TxIdentifier, UTxOIdentifier, ValueDelta,
};
use anyhow::Result;

use crate::{
    immutable_address_store::ImmutableAddressStore, volatile_addresses::VolatileAddresses,
};

#[derive(Debug, Default, Clone)]
pub struct AddressStorageConfig {
    pub db_path: String,
    pub skip_until: Option<u64>,

    pub store_info: bool,
    pub store_totals: bool,
    pub store_transactions: bool,
}

impl AddressStorageConfig {
    pub fn any_enabled(&self) -> bool {
        self.store_info || self.store_totals || self.store_transactions
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, minicbor::Encode, minicbor::Decode)]
pub enum UtxoDelta {
    #[n(0)]
    Created(#[n(0)] UTxOIdentifier),
    #[n(1)]
    Spent(#[n(0)] UTxOIdentifier),
}

#[derive(Debug, Default, Clone)]
pub struct AddressEntry {
    pub utxos: Option<Vec<UtxoDelta>>,
    pub transactions: Option<Vec<TxIdentifier>>,
    pub totals: Option<Vec<ValueDelta>>,
}

#[derive(Clone)]
pub struct State {
    pub config: AddressStorageConfig,
    pub volatile: VolatileAddresses,
    pub immutable: Arc<ImmutableAddressStore>,
}

impl State {
    pub async fn new(config: &AddressStorageConfig) -> Result<Self> {
        let db_path = if Path::new(&config.db_path).is_relative() {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&config.db_path)
        } else {
            PathBuf::from(&config.db_path)
        };

        let store = Arc::new(ImmutableAddressStore::new(&db_path)?);

        let mut config = config.clone();
        config.skip_until = store.get_last_epoch_stored().await?;

        Ok(Self {
            config,
            volatile: VolatileAddresses::default(),
            immutable: store,
        })
    }

    pub async fn get_address_utxos(
        &self,
        address: &Address,
    ) -> Result<Option<Vec<UTxOIdentifier>>> {
        if !self.config.store_info {
            return Err(anyhow::anyhow!("address info storage disabled in config"));
        }

        let store = self.immutable.clone();
        let mut combined: HashSet<UTxOIdentifier> = match store.get_utxos(address).await? {
            Some(db) => db.into_iter().collect(),
            None => HashSet::new(),
        };

        for map in self.volatile.window.iter() {
            if let Some(entry) = map.get(address) {
                if let Some(deltas) = &entry.utxos {
                    for delta in deltas {
                        match delta {
                            UtxoDelta::Created(u) => {
                                combined.insert(*u);
                            }
                            UtxoDelta::Spent(u) => {
                                combined.remove(u);
                            }
                        }
                    }
                }
            }
        }

        if combined.is_empty() {
            Ok(None)
        } else {
            Ok(Some(combined.into_iter().collect()))
        }
    }

    pub async fn get_address_transactions(
        &self,
        address: &Address,
    ) -> Result<Option<Vec<TxIdentifier>>> {
        if !self.config.store_transactions {
            return Err(anyhow::anyhow!(
                "address transactions storage disabled in config"
            ));
        }

        let store = self.immutable.clone();

        let mut combined: Vec<TxIdentifier> = match store.get_txs(address).await? {
            Some(db) => db,
            None => Vec::new(),
        };

        for map in self.volatile.window.iter() {
            if let Some(entry) = map.get(address) {
                if let Some(txs) = &entry.transactions {
                    combined.extend(txs);
                }
            }
        }

        if combined.is_empty() {
            Ok(None)
        } else {
            Ok(Some(combined))
        }
    }

    pub async fn get_address_totals(&self, address: &Address) -> Result<AddressTotals> {
        if !self.config.store_totals {
            anyhow::bail!("address totals storage disabled in config");
        }

        let store = self.immutable.clone();

        let mut totals = match store.get_totals(address).await? {
            Some(db) => db,
            None => AddressTotals::default(),
        };

        for map in self.volatile.window.iter() {
            if let Some(entry) = map.get(address) {
                if let Some(address_deltas) = &entry.totals {
                    for delta in address_deltas {
                        totals.apply_delta(delta);
                    }
                }
            }
        }

        Ok(totals)
    }

    pub async fn prune_volatile(&mut self) {
        let drained = self.volatile.prune_volatile();
        self.immutable.update_immutable(drained).await;
    }

    pub fn ready_to_prune(&self, block_info: &BlockInfo) -> bool {
        block_info.epoch > 0
            && Some(block_info.epoch) != self.volatile.last_persisted_epoch
            && block_info.number > self.volatile.epoch_start_block + self.volatile.security_param_k
    }

    pub fn apply_address_deltas(&mut self, deltas: &[AddressDelta]) -> Result<()> {
        let addresses = self.volatile.window.back_mut().expect("window should never be empty");

        for delta in deltas {
            let entry = addresses.entry(delta.address.clone()).or_default();

            if self.config.store_info {
                let utxos = entry.utxos.get_or_insert(Vec::new());
                if delta.value.lovelace > 0 {
                    utxos.push(UtxoDelta::Created(delta.utxo));
                } else {
                    utxos.push(UtxoDelta::Spent(delta.utxo));
                }
            }

            if self.config.store_transactions {
                let txs = entry.transactions.get_or_insert(Vec::new());
                txs.push(TxIdentifier::from(delta.utxo))
            }

            if self.config.store_totals {
                let totals = entry.totals.get_or_insert(Vec::new());
                totals.push(delta.value.clone());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{Address, AddressDelta, UTxOIdentifier, ValueDelta};
    use tempfile::tempdir;

    fn dummy_address() -> Address {
        Address::from_string("DdzFFzCqrht7fNAHwdou7iXPJ5NZrssAH53yoRMUtF9t6momHH52EAxM5KmqDwhrjT7QsHjbMPJUBywmzAgmF4hj2h9eKj4U6Ahandyy").unwrap()
    }

    fn test_config() -> AddressStorageConfig {
        let dir = tempdir().unwrap();
        AddressStorageConfig {
            db_path: dir.path().to_string_lossy().into_owned(),
            skip_until: None,
            store_info: true,
            store_transactions: true,
            store_totals: true,
        }
    }

    async fn setup_state_and_store() -> Result<State> {
        let config = test_config();
        let mut state = State::new(&config.clone()).await?;
        state.volatile.epoch_start_block = 1;
        Ok(state)
    }

    fn delta(addr: &Address, utxo: &UTxOIdentifier, lovelace: i64) -> AddressDelta {
        AddressDelta {
            address: addr.clone(),
            utxo: utxo.clone(),
            value: ValueDelta {
                lovelace,
                assets: Vec::new(),
            },
        }
    }

    #[tokio::test]
    async fn test_persist_all_and_read_back() -> Result<()> {
        let _ = tracing_subscriber::fmt::try_init();

        let mut state = setup_state_and_store().await?;

        let addr = dummy_address();
        let utxo = UTxOIdentifier::new(0, 0, 0);
        let deltas = vec![delta(&addr, &utxo, 1)];

        // Apply deltas
        state.apply_address_deltas(&deltas)?;

        // Drain volatile to immutable
        state.volatile.epoch_start_block = 1;
        state.prune_volatile().await;

        // Perisist immutable to disk
        state.immutable.persist_epoch(0, &state.config).await?;

        // Verify persisted UTxOs
        let utxos = state.get_address_utxos(&addr).await?;
        assert!(utxos.is_some());
        assert_eq!(utxos.as_ref().unwrap().len(), 1);
        assert_eq!(utxos.as_ref().unwrap()[0], UTxOIdentifier::new(0, 0, 0));

        // Totals should exist
        let totals = state.immutable.get_totals(&addr).await?;
        assert!(totals.is_some());

        // Epoch marker advanced
        let last_epoch = state.immutable.get_last_epoch_stored().await?;
        assert_eq!(last_epoch, Some(0));

        Ok(())
    }

    #[tokio::test]
    async fn test_utxo_removed_when_spent() -> Result<()> {
        let _ = tracing_subscriber::fmt::try_init();

        let mut state = setup_state_and_store().await?;

        let addr = dummy_address();
        let utxo = UTxOIdentifier::new(0, 0, 0);

        // Before processing
        assert!(
            state.get_address_utxos(&addr).await?.is_none(),
            "Expected no UTxOs before creation"
        );

        let created = vec![delta(&addr, &utxo, 1)];

        state.apply_address_deltas(&created)?;

        // After processing creation
        let after_create = state.get_address_utxos(&addr).await?;
        assert_eq!(after_create.as_ref().unwrap(), &[utxo]);

        // Drain volatile to immutable
        state.volatile.epoch_start_block = 1;
        state.prune_volatile().await;

        // Perisist immutable to disk
        state.immutable.persist_epoch(0, &state.config).await?;

        // After persisting creation
        let after_persist = state.get_address_utxos(&addr).await?;
        assert_eq!(after_persist.as_ref().unwrap(), &[utxo]);

        state.volatile.next_block();
        state.apply_address_deltas(&[delta(&addr, &utxo, -1)])?;

        // After processing spend
        let after_spend_volatile = state.get_address_utxos(&addr).await?;
        assert!(after_spend_volatile.as_ref().map_or(true, |u| u.is_empty()));

        // Drain volatile to immutable
        state.prune_volatile().await;

        // Perisist immutable to disk
        state.immutable.persist_epoch(2, &state.config).await?;

        // After persisting spend
        let after_spend_disk = state.get_address_utxos(&addr).await?;
        assert!(after_spend_disk.as_ref().map_or(true, |u| u.is_empty()));

        Ok(())
    }

    #[tokio::test]
    async fn test_utxo_spent_and_created_across_blocks_in_volatile() -> Result<()> {
        let _ = tracing_subscriber::fmt::try_init();

        let mut state = setup_state_and_store().await?;

        let addr = dummy_address();
        let utxo_old = UTxOIdentifier::new(0, 0, 0);
        let utxo_new = UTxOIdentifier::new(0, 1, 0);

        state.volatile.epoch_start_block = 1;

        state.apply_address_deltas(&[delta(&addr, &utxo_old, 1)])?;
        state.volatile.next_block();
        state.apply_address_deltas(&[delta(&addr, &utxo_old, -1), delta(&addr, &utxo_new, 1)])?;

        // Create and spend both in volatile is not included in address utxos
        let volatile = state.get_address_utxos(&addr).await?;
        assert!(
            volatile.as_ref().is_some_and(|u| u.contains(&utxo_new) && !u.contains(&utxo_old)),
            "Expected only new UTxO {:?} in volatile view, got {:?}",
            utxo_new,
            volatile
        );

        // Drain volatile to immutable
        state.prune_volatile().await;

        // Perisist immutable to disk
        state.immutable.persist_epoch(0, &state.config).await?;

        // UTxO not persisted to disk if created and spent in pruned volatile window
        let persisted_view = state.get_address_utxos(&addr).await?;
        assert!(
            persisted_view
                .as_ref()
                .is_some_and(|u| u.contains(&utxo_new) && !u.contains(&utxo_old)),
            "Expected only new UTxO {:?} after persistence, got {:?}",
            utxo_new,
            persisted_view
        );

        Ok(())
    }
}
