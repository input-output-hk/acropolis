use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use acropolis_common::{
    Address, AddressDelta, AddressTotals, BlockInfo, ShelleyAddress, TxIdentifier, UTxOIdentifier,
    ValueDelta, ValueDeltaMap,
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

        let mut combined: Vec<TxIdentifier> = store.get_txs(address).await?.unwrap_or_default();

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

        let mut totals = store.get_totals(address).await?.unwrap_or_default();

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

    pub fn apply_address_deltas(&mut self, deltas: &[AddressDelta]) {
        let addresses = self.volatile.window.back_mut().expect("window should never be empty");

        // Keeps track seen txs to avoid overcounting totals tx count and duplicating tx identifiers
        let mut seen: HashMap<Address, HashSet<TxIdentifier>> = HashMap::new();

        for delta in deltas {
            let tx_id = TxIdentifier::from(delta.utxo);
            let entry = addresses.entry(delta.address.clone()).or_default();

            if self.config.store_info {
                let utxos = entry.utxos.get_or_insert(Vec::new());
                if delta.value.lovelace > 0 {
                    utxos.push(UtxoDelta::Created(delta.utxo));
                } else {
                    utxos.push(UtxoDelta::Spent(delta.utxo));
                }
            }

            if self.config.store_transactions || self.config.store_totals {
                let seen_for_addr = seen.entry(delta.address.clone()).or_default();

                if self.config.store_transactions {
                    let txs = entry.transactions.get_or_insert(Vec::new());
                    if !seen_for_addr.contains(&tx_id) {
                        txs.push(tx_id);
                    }
                }
                if self.config.store_totals {
                    let totals = entry.totals.get_or_insert(Vec::new());

                    if seen_for_addr.contains(&tx_id) {
                        if let Some(last_total) = totals.last_mut() {
                            // Create temporary map for summing same tx deltas efficiently
                            // TODO: Potentially move upstream to address deltas publisher
                            let mut map = ValueDeltaMap::from(last_total.clone());
                            map += delta.value.clone();
                            *last_total = ValueDelta::from(map);
                        }
                    } else {
                        totals.push(delta.value.clone());
                    }
                }
                seen_for_addr.insert(tx_id);
            }
        }
    }

    pub async fn get_addresses_totals(
        &self,
        addresses: &[ShelleyAddress],
    ) -> Result<AddressTotals> {
        let mut totals = AddressTotals::default();
        for addr in addresses {
            totals += self.get_address_totals(&Address::Shelley(addr.clone())).await?;
        }
        Ok(totals)
    }

    pub async fn get_addresses_utxos(
        &self,
        addresses: &[ShelleyAddress],
    ) -> Result<Vec<UTxOIdentifier>> {
        let mut utxos = Vec::new();

        for addr in addresses {
            if let Some(list) = self.get_address_utxos(&Address::Shelley(addr.clone())).await? {
                utxos.extend(list);
            }
        }
        Ok(utxos)
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
            utxo: *utxo,
            value: ValueDelta {
                lovelace,
                assets: Vec::new(),
            },
        }
    }

    #[tokio::test]
    async fn test_utxo_storage_lifecycle() -> Result<()> {
        let _ = tracing_subscriber::fmt::try_init();

        let mut state = setup_state_and_store().await?;

        let addr = dummy_address();
        let utxo = UTxOIdentifier::new(0, 0, 0);
        let deltas = vec![delta(&addr, &utxo, 1)];

        // Apply deltas
        state.apply_address_deltas(&deltas);

        // Verify UTxO is retrievable when in volatile
        let utxos = state.get_address_utxos(&addr).await?;
        assert!(utxos.is_some());
        assert_eq!(utxos.as_ref().unwrap().len(), 1);
        assert_eq!(utxos.as_ref().unwrap()[0], UTxOIdentifier::new(0, 0, 0));

        // Drain volatile to immutable
        state.volatile.epoch_start_block = 1;
        state.prune_volatile().await;

        // Verify UTxO is retrievable when in immutable pending
        let utxos = state.get_address_utxos(&addr).await?;
        assert!(utxos.is_some());
        assert_eq!(utxos.as_ref().unwrap().len(), 1);
        assert_eq!(utxos.as_ref().unwrap()[0], UTxOIdentifier::new(0, 0, 0));

        // Perisist immutable to disk
        state.immutable.persist_epoch(0, &state.config).await?;

        // Verify UTxO is retrievable after persisted to disk
        let utxos = state.get_address_utxos(&addr).await?;
        assert!(utxos.is_some());
        assert_eq!(utxos.as_ref().unwrap().len(), 1);
        assert_eq!(utxos.as_ref().unwrap()[0], UTxOIdentifier::new(0, 0, 0));

        Ok(())
    }

    #[tokio::test]
    async fn test_utxo_removed_when_spent() -> Result<()> {
        let _ = tracing_subscriber::fmt::try_init();

        let mut state = setup_state_and_store().await?;

        let addr = dummy_address();
        let utxo = UTxOIdentifier::new(0, 0, 0);

        let created = vec![delta(&addr, &utxo, 1)];

        // Apply delta to volatile
        state.apply_address_deltas(&created);

        // Drain volatile to immutable pending
        state.volatile.epoch_start_block = 1;
        state.prune_volatile().await;

        // Perisist immutable to disk
        state.immutable.persist_epoch(0, &state.config).await?;

        // Verify UTxO was persisted
        let after_persist = state.get_address_utxos(&addr).await?;
        assert_eq!(after_persist.as_ref().unwrap(), &[utxo]);

        state.volatile.next_block();
        state.apply_address_deltas(&[delta(&addr, &utxo, -1)]);

        // Verify UTxO was removed while in volatile
        let after_spend_volatile = state.get_address_utxos(&addr).await?;
        assert!(after_spend_volatile.as_ref().is_none_or(|u| u.is_empty()));

        // Drain volatile to immutable
        state.prune_volatile().await;

        // Verify UTxO was removed while in pending immutable
        let after_spend_pending = state.get_address_utxos(&addr).await?;
        assert!(after_spend_pending.as_ref().is_none_or(|u| u.is_empty()));

        // Perisist immutable to disk
        state.immutable.persist_epoch(2, &state.config).await?;

        // Verify UTxO was removed after persisting spend to disk
        let after_spend_disk = state.get_address_utxos(&addr).await?;
        assert!(after_spend_disk.as_ref().is_none_or(|u| u.is_empty()));

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

        state.apply_address_deltas(&[delta(&addr, &utxo_old, 1)]);
        state.volatile.next_block();
        state.apply_address_deltas(&[delta(&addr, &utxo_old, -1), delta(&addr, &utxo_new, 1)]);

        // Verify Create and spend both in volatile is not included in address utxos
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

    #[tokio::test]
    async fn test_same_tx_deltas_sums_totals_in_volatile() -> Result<()> {
        let _ = tracing_subscriber::fmt::try_init();

        let mut state = setup_state_and_store().await?;

        let addr = dummy_address();
        let delta_1 = UTxOIdentifier::new(0, 1, 0);
        let delta_2 = UTxOIdentifier::new(0, 1, 1);

        state.volatile.epoch_start_block = 1;

        state.apply_address_deltas(&[delta(&addr, &delta_1, 1), delta(&addr, &delta_2, 1)]);

        // Verify only 1 totals entry with delta of 2
        let volatile = state
            .volatile
            .window
            .back()
            .expect("Window should have a delta")
            .get(&addr)
            .expect("Entry should be populated")
            .totals
            .as_ref()
            .expect("Totals should be populated");

        assert_eq!(volatile.len(), 1);
        assert_eq!(volatile.first().expect("Should be populated").lovelace, 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_same_tx_deltas_prevents_duplicate_identifier_in_volatile() -> Result<()> {
        let _ = tracing_subscriber::fmt::try_init();

        let mut state = setup_state_and_store().await?;

        let addr = dummy_address();
        let delta_1 = UTxOIdentifier::new(0, 1, 0);
        let delta_2 = UTxOIdentifier::new(0, 1, 1);

        state.volatile.epoch_start_block = 1;

        state.apply_address_deltas(&[delta(&addr, &delta_1, 1), delta(&addr, &delta_2, 1)]);

        // Verify only 1 transactions entry
        let volatile = state
            .volatile
            .window
            .back()
            .expect("Window should have a delta")
            .get(&addr)
            .expect("Entry should be populated")
            .transactions
            .as_ref()
            .expect("Transactions should be populated");

        assert_eq!(volatile.len(), 1);

        Ok(())
    }
}
