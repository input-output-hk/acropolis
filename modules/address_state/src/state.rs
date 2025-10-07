use std::collections::HashSet;

use acropolis_common::{
    Address, AddressDelta, AddressTotals, TxIdentifier, UTxOIdentifier, ValueDelta,
};
use anyhow::Result;

use crate::{address_store::AddressStore, volatile_index::VolatileIndex};

#[derive(Debug, Default, Clone, Copy)]
pub struct AddressStorageConfig {
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

#[derive(Debug, Default, Clone)]
pub struct State {
    pub config: AddressStorageConfig,
    pub volatile_entries: VolatileIndex,
}

impl State {
    pub fn new(config: AddressStorageConfig) -> Self {
        Self {
            config,
            volatile_entries: VolatileIndex::default(),
        }
    }

    pub async fn get_address_utxos(
        &self,
        store: &dyn AddressStore,
        address: &Address,
    ) -> Result<Option<Vec<UTxOIdentifier>>> {
        if !self.config.store_info {
            return Err(anyhow::anyhow!("address info storage disabled in config"));
        }

        let mut combined: HashSet<UTxOIdentifier> = match store.get_utxos(address)? {
            Some(db) => db.into_iter().collect(),
            None => HashSet::new(),
        };

        for map in self.volatile_entries.window.iter() {
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
        store: &dyn AddressStore,
        address: &Address,
    ) -> Result<Option<Vec<TxIdentifier>>> {
        if !self.config.store_transactions {
            return Err(anyhow::anyhow!(
                "address transactions storage disabled in config"
            ));
        }

        let mut combined: Vec<TxIdentifier> = match store.get_txs(address).await? {
            Some(db) => db,
            None => Vec::new(),
        };

        for map in self.volatile_entries.window.iter() {
            if let Some(entry) = map.get(address) {
                if let Some(txs) = &entry.transactions {
                    combined.extend(txs.iter().cloned());
                }
            }
        }

        if combined.is_empty() {
            Ok(None)
        } else {
            Ok(Some(combined))
        }
    }

    pub async fn get_address_totals(
        &self,
        store: &dyn AddressStore,
        address: &Address,
    ) -> Result<AddressTotals> {
        if !self.config.store_totals {
            anyhow::bail!("address totals storage disabled in config");
        }

        let mut totals = match store.get_totals(address).await? {
            Some(db) => db,
            None => AddressTotals::default(),
        };

        for map in self.volatile_entries.window.iter() {
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

    pub fn handle_address_deltas(&mut self, deltas: &[AddressDelta]) -> Result<()> {
        let addresses = self
            .volatile_entries
            .window
            .back_mut()
            .expect("next_block() must be called before handle_address_deltas");

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
