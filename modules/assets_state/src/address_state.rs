use crate::{asset_registry::AssetId, AssetRegistry};
use acropolis_common::{
    params::SECURITY_PARAMETER_K, Address, AddressDelta, AssetAddressEntry, ShelleyAddress,
};
use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use tracing::error;

#[derive(Debug, PartialEq)]
pub struct AddressState {
    // Assets mapped to addresses
    pub addresses: HashMap<AssetId, HashMap<ShelleyAddress, u64>>,

    // History of deltas applied for previous blocks
    delta_history: VecDeque<Vec<AddressDelta>>,

    // History of new assets added in previous blocks
    asset_history: VecDeque<Vec<AssetId>>,

    // Last block number for detecting rollbacks
    last_block_num: Option<u64>,
}

impl AddressState {
    pub fn new() -> Self {
        Self {
            addresses: HashMap::new(),
            delta_history: VecDeque::new(),
            asset_history: VecDeque::new(),
            last_block_num: None,
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

    fn rollback(&mut self, num_blocks: u64, registry: &AssetRegistry) {
        for _ in 0..num_blocks {
            if let Some(deltas) = self.delta_history.pop_back() {
                if self
                    .apply_address_deltas(
                        deltas
                            .into_iter()
                            .map(|mut d| {
                                (d.sent, d.received) = (d.received, d.sent);
                                d
                            })
                            .collect::<Vec<_>>()
                            .as_slice(),
                        registry,
                    )
                    .is_err()
                {
                    error!("Failed to apply reversed deltas during rollback");
                }
            }
            if let Some(ids) = self.asset_history.pop_back() {
                for id in ids {
                    self.addresses.remove(&id);
                }
            }
        }
        if let Some(last_block_num) = self.last_block_num {
            if num_blocks < last_block_num {
                self.last_block_num = Some(last_block_num - num_blocks);
            } else {
                self.last_block_num = None;
            }
        }
    }

    pub fn new_block(&mut self, block_num: u64, ids: &[AssetId], registry: &AssetRegistry) {
        if let Some(last_block_num) = self.last_block_num {
            if last_block_num >= block_num {
                self.rollback(last_block_num - block_num + 1, registry);
            } else if block_num > last_block_num + 1 {
                error!("Block(s) skipped - rollbacks may produce unexpected results");
            }
        }
        self.last_block_num = Some(block_num);
        let mut added = Vec::new();
        for id in ids {
            self.addresses.entry(*id).or_insert_with(|| {
                added.push(*id);
                HashMap::new()
            });
        }
        self.asset_history.push_back(added);
        if self.asset_history.len() > SECURITY_PARAMETER_K as usize {
            self.asset_history.pop_front();
        }
    }

    fn apply_address_deltas(
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
        Ok(())
    }

    pub fn handle_address_deltas(
        &mut self,
        deltas: &[AddressDelta],
        registry: &AssetRegistry,
    ) -> Result<()> {
        self.apply_address_deltas(deltas, registry)?;

        self.delta_history.push_back(deltas.to_vec());
        if self.delta_history.len() > SECURITY_PARAMETER_K as usize {
            self.delta_history.pop_front();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use acropolis_common::{AssetName, NativeAsset, PolicyId, TxIdentifier, Value};

    fn make_address_delta(
        address: &Address,
        policy_id: PolicyId,
        name: AssetName,
        sent_amount: u64,
        received_amount: u64,
    ) -> AddressDelta {
        AddressDelta {
            address: address.clone(),
            tx_identifier: TxIdentifier::new(0, 0),
            spent_utxos: Vec::new(),
            created_utxos: Vec::new(),

            sent: Value::new(
                0,
                if sent_amount > 0 {
                    vec![(
                        policy_id,
                        vec![NativeAsset {
                            name,
                            amount: sent_amount,
                        }],
                    )]
                } else {
                    vec![]
                },
            ),

            received: Value::new(
                0,
                if received_amount > 0 {
                    vec![(
                        policy_id,
                        vec![NativeAsset {
                            name,
                            amount: received_amount,
                        }],
                    )]
                } else {
                    vec![]
                },
            ),
        }
    }

    fn shelley_address() -> acropolis_common::ShelleyAddress {
        ShelleyAddress::from_string(
            "addr1q9g0u0aeuyvrn8ptc6yesgj6dtfgw2gadnc9y2p9cs8pneejrkwtdvk97yp2zayhvmm3wu0v672psdg2xn0temkz83ds7qfxdt",
        )
        .unwrap()
    }

    fn shelley_address2() -> acropolis_common::ShelleyAddress {
        ShelleyAddress::from_string(
            "addr1qxt38qvkareq8yqsdtqz6m5amqqs2e3nc0sm557064hz25j5dmcve8eyjgeq5yn004xztx4h28zup08jtqyp76s5nsnsal8fdp",
        )
        .unwrap()
    }

    fn shelley_address3() -> acropolis_common::ShelleyAddress {
        ShelleyAddress::from_string(
            "addr1q9ef7tp8q29egx68cv0t4kcfgudtqnafg9tf8eymfzxy2zxpnx2hmpwppvm7sy3v6unum9tp8a4gl305w044dt73m05qcg5f8f",
        )
        .unwrap()
    }

    fn dummy_address() -> acropolis_common::Address {
        acropolis_common::Address::Shelley(shelley_address())
    }

    fn dummy_address2() -> acropolis_common::Address {
        acropolis_common::Address::Shelley(shelley_address2())
    }

    #[test]
    fn handle_address_deltas_accumulates_address_balance() {
        let mut registry = AssetRegistry::new();
        let policy_id = PolicyId::from([5u8; 28]);
        let asset_name = AssetName::new(b"TKN").unwrap();
        let asset_id = registry.get_or_insert(policy_id, asset_name);

        let mut state = AddressState::new();
        state.addresses.insert(asset_id, HashMap::new());

        let address = dummy_address();
        let delta1 = make_address_delta(&address, policy_id, asset_name, 0, 10);
        let delta2 = make_address_delta(&address, policy_id, asset_name, 0, 15);

        state.handle_address_deltas(&[delta1, delta2], &registry).unwrap();
        let holders = state.addresses.get(&asset_id).unwrap();

        // Sum of both deltas applied to address balance
        assert_eq!(
            *holders
                .get(match &dummy_address() {
                    Address::Shelley(s) => s,
                    _ => panic!(),
                })
                .unwrap(),
            25
        );
    }

    #[test]
    fn handle_address_deltas_removes_zero_balance_addresses() {
        let mut registry = AssetRegistry::new();
        let policy_id = PolicyId::from([5u8; 28]);
        let asset_name = AssetName::new(b"TKN").unwrap();
        let asset_id = registry.get_or_insert(policy_id, asset_name);

        let mut state = AddressState::new();
        state.addresses.insert(asset_id, HashMap::new());

        let address = dummy_address();
        let add_delta = make_address_delta(&address, policy_id, asset_name, 0, 10);
        state.handle_address_deltas(&[add_delta], &registry).unwrap();
        let holders = state.addresses.get(&asset_id).unwrap();

        // Address added to asset map with correct balance
        assert_eq!(
            *holders
                .get(match &dummy_address() {
                    Address::Shelley(s) => s,
                    _ => panic!(),
                })
                .unwrap(),
            10
        );

        let remove_delta = make_address_delta(&address, policy_id, asset_name, 10, 0);
        state.handle_address_deltas(&[remove_delta], &registry).unwrap();
        let holders = state.addresses.get(&asset_id).unwrap();

        // Address removed from asset map after balance zeroed
        assert!(!holders.contains_key(match &dummy_address() {
            Address::Shelley(s) => s,
            _ => panic!(),
        }));
    }

    #[test]
    fn test_rollback() -> Result<(), Box<dyn std::error::Error>> {
        let mut registry = AssetRegistry::new();
        let policy_id = PolicyId::from([5u8; 28]);
        let asset_name1 = AssetName::new(b"TKN").unwrap();
        let asset_id1 = registry.get_or_insert(policy_id, asset_name1);
        let asset_name2 = AssetName::new(b"FOO").unwrap();
        let asset_id2 = registry.get_or_insert(policy_id, asset_name2);
        let asset_name3 = AssetName::new(b"BAR").unwrap();
        let asset_id3 = registry.get_or_insert(policy_id, asset_name3);
        let shelley1 = shelley_address();
        let address1 = dummy_address();
        let shelley2 = shelley_address2();
        let address2 = dummy_address2();
        let shelley3 = shelley_address3();

        let mut state = AddressState {
            addresses: HashMap::from([
                (asset_id1, HashMap::from([(shelley1.clone(), 100)])),
                (asset_id2, HashMap::from([(shelley2.clone(), 200)])),
                (asset_id3, HashMap::from([(shelley3, 300)])),
            ]),
            asset_history: VecDeque::from([
                Vec::from([]),
                Vec::from([]),
                Vec::from([]),
                Vec::from([asset_id3]),
                Vec::from([]),
                Vec::from([]),
            ]),
            delta_history: VecDeque::from([
                Vec::from([]),
                Vec::from([]),
                Vec::from([]),
                Vec::from([]),
                Vec::from([]),
                Vec::from([
                    make_address_delta(&address1, policy_id, asset_name1, 100, 0),
                    make_address_delta(&address2, policy_id, asset_name2, 0, 80),
                ]),
            ]),
            last_block_num: Some(10),
        };
        state.rollback(3, &registry);
        assert_eq!(
            AddressState {
                addresses: HashMap::from([
                    (asset_id1, HashMap::from([(shelley1, 200)])),
                    (asset_id2, HashMap::from([(shelley2, 120)])),
                ]),
                asset_history: VecDeque::from([Vec::from([]), Vec::from([]), Vec::from([]),]),
                delta_history: VecDeque::from([Vec::from([]), Vec::from([]), Vec::from([]),]),
                last_block_num: Some(7),
            },
            state
        );
        Ok(())
    }
}
