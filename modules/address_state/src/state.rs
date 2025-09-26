use acropolis_common::{Address, AddressDelta, AddressTotalsEntry, TxIdentifier, UTxOIdentifier};
use anyhow::Result;
use imbl::{HashMap, Vector};

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

#[derive(Debug, Default, Clone)]
pub struct AddressEntry {
    pub utxos: Option<HashMap<UTxOIdentifier, ()>>,
    pub transactions: Option<Vector<TxIdentifier>>,
    pub totals: Option<AddressTotalsEntry>,
}

#[derive(Debug, Default, Clone)]
pub struct State {
    pub config: AddressStorageConfig,

    pub addresses: Option<HashMap<Address, AddressEntry>>,
}

impl State {
    pub fn new(config: AddressStorageConfig) -> Self {
        Self {
            config,
            addresses: if config.any_enabled() {
                Some(HashMap::new())
            } else {
                None
            },
        }
    }

    pub fn get_address_utxos(&self, address: &Address) -> Result<Option<Vec<UTxOIdentifier>>> {
        if !self.config.store_info {
            anyhow::bail!("address info storage disabled in config");
        }

        Ok(self
            .addresses
            .as_ref()
            .and_then(|map| map.get(address))
            .and_then(|entry| entry.utxos.as_ref())
            .map(|m| m.keys().cloned().collect()))
    }

    pub fn get_address_totals(&self, address: &Address) -> Result<AddressTotalsEntry> {
        if !self.config.store_totals {
            anyhow::bail!("address totals storage disabled in config");
        }

        self.addresses
            .as_ref()
            .and_then(|map| map.get(address))
            .and_then(|entry| entry.totals.clone())
            .ok_or_else(|| anyhow::anyhow!("address not initialized in totals map"))
    }

    pub fn get_address_transactions(&self, address: &Address) -> Result<Option<Vec<TxIdentifier>>> {
        if !self.config.store_transactions {
            anyhow::bail!("address transactions storage disabled in config");
        }

        Ok(self
            .addresses
            .as_ref()
            .and_then(|map| map.get(address))
            .and_then(|entry| entry.transactions.as_ref())
            .map(|v| v.iter().cloned().collect()))
    }

    pub fn tick(&self) -> Result<()> {
        let count = self.addresses.as_ref().map(|m| m.len()).unwrap_or(0);

        if count != 0 {
            tracing::info!("Tracking {} addresses", count);
        } else {
            tracing::info!("address_state storage disabled in config");
        }

        Ok(())
    }

    pub fn handle_address_deltas(&self, deltas: &[AddressDelta]) -> Result<Self> {
        let mut new_state = self.clone();

        let Some(addresses) = new_state.addresses.as_mut() else {
            return Ok(new_state);
        };

        for delta in deltas {
            let entry = addresses.entry(delta.address.clone()).or_default();

            if self.config.store_info {
                let utxos = entry.utxos.get_or_insert_with(HashMap::new);
                if delta.value.lovelace > 0 {
                    utxos.insert(delta.utxo, ());
                } else {
                    utxos.remove(&delta.utxo);
                }
            }

            if self.config.store_transactions {
                let transactions = entry.transactions.get_or_insert_with(Vector::new);

                let tx_id = delta.utxo.to_tx_identifier();

                if transactions.last() != Some(&tx_id) {
                    transactions.push_back(tx_id);
                }
            }
        }
        Ok(new_state)
    }
}
