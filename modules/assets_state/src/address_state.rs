use crate::{asset_registry::AssetId, AssetRegistry};
use acropolis_common::{
    params::SECURITY_PARAMETER_K, Address, AddressDelta, AssetAddressEntry, ShelleyAddress,
};
use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use tracing::error;

pub struct AddressState {
    // Assets mapped to addresses
    pub addresses: HashMap<AssetId, HashMap<ShelleyAddress, u64>>,

    history: VecDeque<Vec<AddressDelta>>,
}

impl AddressState {
    pub fn new() -> Self {
        Self {
            addresses: HashMap::new(),
            history: VecDeque::new(),
        }
    }

    pub fn get_asset_addresses(
        &self,
        asset_id: &AssetId,
    ) -> Result<Option<Vec<AssetAddressEntry>>> {
        Ok(self.addresses.get(asset_id).map(|inner_map| {
            inner_map
                .iter()
                .map(|(addr, qty)| AssetAddressEntry {
                    address: addr.clone(),
                    quantity: *qty,
                })
                .collect()
        }))
    }

    pub fn add_asset_ids(&mut self, ids: &[AssetId]) {
        for id in ids {
            self.addresses.entry(*id).or_default();
        }
        // TODO: record actual additions for rolling back
    }

    pub fn handle_address_deltas(
        &mut self,
        deltas: &[AddressDelta],
        registry: &AssetRegistry,
    ) -> Result<()> {
        for address_delta in deltas {
            if let Address::Shelley(shelley_addr) = &address_delta.address {
                for (policy_id, assets) in &address_delta.sent.assets {
                    for asset in assets {
                        if let Some(asset_id) = registry.lookup_id(policy_id, &asset.name) {
                            if let Some(holders) = self.addresses.get_mut(&asset_id) {
                                let current = holders.entry(shelley_addr.clone()).or_insert(0);
                                *current = current.saturating_sub(asset.amount);

                                if *current == 0 {
                                    holders.remove(shelley_addr);
                                }
                            } else {
                                error!("Sent delta for unknown asset_id: {:?}", asset_id);
                            }
                        }
                    }
                }

                for (policy_id, assets) in &address_delta.received.assets {
                    for asset in assets {
                        if let Some(asset_id) = registry.lookup_id(policy_id, &asset.name) {
                            if let Some(holders) = self.addresses.get_mut(&asset_id) {
                                let current = holders.entry(shelley_addr.clone()).or_insert(0);
                                *current = current.saturating_add(asset.amount);
                            } else {
                                error!("Received delta for unknown asset_id: {:?}", asset_id);
                            }
                        }
                    }
                }
            }
        }

        self.history.push_back(deltas.to_vec());
        if self.history.len() > SECURITY_PARAMETER_K as usize {
            self.history.pop_front();
        }

        Ok(())
    }
}
