use acropolis_common::{AddressDelta, AddressTotalsEntry, TxHash, UTXODelta, UTxOIdentifier};
use anyhow::Result;
use imbl::{HashMap, Vector};

use crate::address_registry::{AddressId, AddressRegistry};

#[derive(Debug, Default, Clone, Copy)]
pub struct AddressStorageConfig {
    pub enable_registry: bool,
    pub store_info: bool,
    pub store_totals: bool,
    pub store_transactions: bool,
    pub index_utxos_by_asset: bool,
}

impl AddressStorageConfig {
    pub fn any_enabled(&self) -> bool {
        self.enable_registry
            || self.store_info
            || self.store_totals
            || self.store_transactions
            || self.index_utxos_by_asset
    }
}

#[derive(Debug, Default, Clone)]
pub struct State {
    pub config: AddressStorageConfig,

    /// Addresses mapped to utxos
    pub utxos: Option<HashMap<AddressId, Vector<UTxOIdentifier>>>,

    /// Addresses mapped to sent / receieved totals
    pub totals: Option<HashMap<AddressId, AddressTotalsEntry>>,

    /// Index of UTxOs by (address, asset)
    pub asset_index: Option<HashMap<(AddressId, u64), Vector<UTxOIdentifier>>>,

    /// Addresses mapped to transactions
    pub transactions: Option<HashMap<AddressId, Vector<TxHash>>>,
}

impl State {
    pub fn new(config: AddressStorageConfig) -> Self {
        let store_info = config.store_info;
        let store_totals = config.store_totals;
        let store_transactions = config.store_transactions;
        let index_utxos_by_asset = config.index_utxos_by_asset;

        Self {
            config,
            utxos: if store_info {
                Some(HashMap::new())
            } else {
                None
            },
            totals: if store_totals {
                Some(HashMap::new())
            } else {
                None
            },
            asset_index: if index_utxos_by_asset {
                Some(HashMap::new())
            } else {
                None
            },
            transactions: if store_transactions {
                Some(HashMap::new())
            } else {
                None
            },
        }
    }

    pub fn get_address_utxos(&self, address_id: &AddressId) -> Result<Option<Vec<UTxOIdentifier>>> {
        if !self.config.store_info {
            return Err(anyhow::anyhow!("address info storage disabled in config"));
        }
        Ok(
            self.utxos
                .as_ref()
                .and_then(|m| m.get(address_id))
                .map(|v| v.iter().cloned().collect()),
        )
    }

    pub fn get_address_totals(&self, id: &AddressId) -> Result<AddressTotalsEntry> {
        if !self.config.store_totals {
            return Err(anyhow::anyhow!("address totals storage disabled in config"));
        }

        self.totals
            .as_ref()
            .and_then(|m| m.get(id).cloned())
            .ok_or_else(|| anyhow::anyhow!("address not initialized in totals map"))
    }

    pub fn get_address_asset_utxos(
        &self,
        address_id: &AddressId,
        asset_id: u64,
    ) -> Result<Option<Vec<UTxOIdentifier>>> {
        if !self.config.index_utxos_by_asset {
            return Err(anyhow::anyhow!("asset index storage disabled in config"));
        }

        Ok(self
            .asset_index
            .as_ref()
            .and_then(|m| m.get(&(*address_id, asset_id)))
            .map(|v| v.iter().cloned().collect()))
    }

    pub fn get_address_transactions(&self, address_id: &AddressId) -> Result<Option<Vec<TxHash>>> {
        if !self.config.store_transactions {
            return Err(anyhow::anyhow!(
                "address transactions storage disabled in config"
            ));
        }

        Ok(self
            .transactions
            .as_ref()
            .and_then(|m| m.get(address_id))
            .map(|v| v.iter().cloned().collect()))
    }

    pub fn tick(&self) -> Result<()> {
        let count = if let Some(m) = &self.utxos {
            m.len()
        } else if let Some(m) = &self.totals {
            m.len()
        } else if let Some(m) = &self.transactions {
            m.len()
        } else if let Some(m) = &self.asset_index {
            let unique: std::collections::HashSet<_> = m.keys().map(|(addr, _)| *addr).collect();
            unique.len()
        } else {
            0
        };

        if count != 0 {
            tracing::info!("Tracking {} addresses", count);
        } else {
            tracing::info!("address_state storage disabled in config");
        }
        Ok(())
    }

    pub fn handle_address_deltas(
        &self,
        deltas: &[AddressDelta],
        registry: &mut AddressRegistry,
    ) -> Result<Self> {
        let mut new_totals = self.totals.clone();
        for delta in deltas {
            let address_id = registry.get_or_insert(delta.address);

            if let Some(ref mut totals_map) = new_totals {
                totals_map
                    .entry(address_id)
                    .and_modify(|v| *v += delta.delta.clone())
                    .or_insert(delta.delta.clone());
            }
        }
        Ok(self.clone())
    }
}
