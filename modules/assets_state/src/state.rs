//! Acropolis AssetsState: State storage

use crate::asset_registry::{AssetId, AssetRegistry};
use acropolis_common::{
    queries::assets::{AssetHistory, AssetInfoRecord, AssetMintRecord, PolicyAsset, PolicyAssets},
    NativeAssetDelta, PolicyId, ShelleyAddress, TxHash,
};
use anyhow::Result;
use imbl::{HashMap, Vector};
use tracing::info;

#[derive(Debug, Default, Clone, Copy)]
pub struct AssetsStorageConfig {
    pub store_assets: bool,
    pub store_info: bool,
    pub store_history: bool,
    pub store_transactions: bool,
    pub store_addresses: bool,
    pub index_by_policy: bool,
}

#[derive(Debug, Default, Clone)]
pub struct State {
    pub config: AssetsStorageConfig,

    /// Assets mapped to supply
    pub supply: Option<HashMap<AssetId, u64>>,

    /// Assets mapped to mint/burn history
    pub history: Option<HashMap<AssetId, Vector<AssetMintRecord>>>,

    /// Assets mapped to extended info
    pub info: Option<HashMap<AssetId, AssetInfoRecord>>,

    /// Assets mapped to addresses
    pub addresses: Option<HashMap<AssetId, Vector<(ShelleyAddress, u64)>>>,

    /// Assets mapped to transactions
    pub transactions: Option<HashMap<AssetId, Vector<TxHash>>>,

    // PolicyId mapped associated AssetIds
    pub policy_index: Option<HashMap<PolicyId, Vector<AssetId>>>,
}

impl State {
    pub fn new(config: AssetsStorageConfig) -> Self {
        let store_assets = config.store_assets;
        let store_history = config.store_history;
        let store_info = config.store_info;
        let store_addresses = config.store_addresses;
        let store_transactions = config.store_transactions;
        let index_by_policy = config.index_by_policy;

        Self {
            config,
            supply: if store_assets {
                Some(HashMap::new())
            } else {
                None
            },
            history: if store_history {
                Some(HashMap::new())
            } else {
                None
            },
            info: if store_info {
                Some(HashMap::new())
            } else {
                None
            },
            addresses: if store_addresses {
                Some(HashMap::new())
            } else {
                None
            },
            transactions: if store_transactions {
                Some(HashMap::new())
            } else {
                None
            },
            policy_index: if index_by_policy {
                Some(HashMap::new())
            } else {
                None
            },
        }
    }

    pub fn get_assets_list(&self, registry: &AssetRegistry) -> Result<Vec<PolicyAsset>> {
        let supply = self
            .supply
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("asset storage is disabled in config"))?;

        let mut out = Vec::with_capacity(supply.len());
        for (id, amount) in supply {
            if let Some(key) = registry.lookup(*id) {
                out.push(PolicyAsset {
                    policy: *key.policy,
                    name: (*key.name).clone(),
                    quantity: *amount,
                });
            }
        }

        Ok(out)
    }

    pub fn get_asset_info(&self, asset_id: &AssetId) -> Result<Option<(u64, AssetInfoRecord)>> {
        if !self.config.store_info {
            return Err(anyhow::anyhow!("asset info storage disabled in config"));
        }

        let supply = self.supply.as_ref().and_then(|supply_map| supply_map.get(asset_id));

        let info = self.info.as_ref().and_then(|info_map| info_map.get(asset_id));

        Ok(match (supply, info) {
            (Some(supply), Some(info)) => Some((*supply, info.clone())),
            _ => None,
        })
    }

    pub fn get_asset_history(&self, asset_id: &AssetId) -> Result<Option<AssetHistory>> {
        if !self.config.store_history {
            return Err(anyhow::anyhow!("asset history storage disabled in config"));
        }

        Ok(self
            .history
            .as_ref()
            .and_then(|hist_map| hist_map.get(asset_id))
            .map(|v| v.iter().cloned().collect()))
    }

    pub fn get_asset_addresses(
        &self,
        asset_id: &AssetId,
    ) -> Result<Option<Vec<(ShelleyAddress, u64)>>> {
        if !self.config.store_addresses {
            return Err(anyhow::anyhow!(
                "asset addresses storage disabled in config"
            ));
        }

        Ok(self
            .addresses
            .as_ref()
            .and_then(|addr_map| addr_map.get(asset_id))
            .map(|v| v.iter().cloned().collect()))
    }

    pub fn get_asset_transactions(&self, asset_id: &AssetId) -> Result<Option<Vec<TxHash>>> {
        if !self.config.store_transactions {
            return Err(anyhow::anyhow!(
                "asset transactions storage disabled in config"
            ));
        }

        Ok(self
            .transactions
            .as_ref()
            .and_then(|tx_map| tx_map.get(asset_id))
            .map(|v| v.iter().cloned().collect()))
    }

    pub fn get_policy_assets(
        &self,
        policy_id: &PolicyId,
        registry: &AssetRegistry,
    ) -> Result<Option<PolicyAssets>> {
        if !self.config.store_assets {
            return Err(anyhow::anyhow!("asset storage is disabled in config"));
        }
        if !self.config.index_by_policy {
            return Err(anyhow::anyhow!("policy index disabled in config"));
        }

        let ids = match self.policy_index.as_ref().and_then(|idx| idx.get(policy_id)) {
            Some(ids) => ids,
            None => return Ok(None),
        };

        let supply_map = self.supply.as_ref();

        let result: Vec<PolicyAsset> = ids
            .iter()
            .filter_map(|asset_id| {
                let supply = supply_map.and_then(|s| s.get(asset_id))?;
                let key = registry.lookup(*asset_id)?;
                Some(PolicyAsset {
                    policy: *policy_id,
                    name: (*key.name).clone(),
                    quantity: *supply,
                })
            })
            .collect();

        Ok(Some(result))
    }

    pub fn tick(&self) -> Result<()> {
        match (&self.supply, &self.policy_index) {
            (Some(supply), Some(policy_index)) => {
                let asset_count = supply.len();
                let policy_count = policy_index.len();
                info!(
                    asset_count,
                    policy_count,
                    "Tracking {policy_count} policy ids containing {asset_count} assets"
                );
            }
            (Some(supply), None) => {
                let asset_count = supply.len();
                info!(asset_count, "Tracking {asset_count} assets");
            }
            _ => {
                info!("asset_state storage disabled in config");
            }
        }

        Ok(())
    }

    pub fn handle_mint_deltas(
        &self,
        deltas: &[(TxHash, Vec<(PolicyId, Vec<NativeAssetDelta>)>)],
        registry: &mut AssetRegistry,
    ) -> Result<Self> {
        let mut new_supply = self.supply.clone();
        let mut new_info = self.info.clone();
        let mut new_history = self.history.clone();
        let mut new_index = self.policy_index.clone();
        let mut new_addresses = self.addresses.clone();
        let mut new_transactions = self.transactions.clone();

        for (tx_hash, tx_deltas) in deltas {
            for (policy_id, asset_deltas) in tx_deltas {
                for delta in asset_deltas {
                    let asset_id = registry.get_or_insert(*policy_id, delta.name.clone());

                    if let Some(supply) = new_supply.as_mut() {
                        let delta_amount = delta.amount;

                        let new_amt = match supply.get(&asset_id) {
                            Some(&current) => {
                                let sum = (current as i128) + (delta_amount as i128);
                                u64::try_from(sum).map_err(|_| {
                                    anyhow::anyhow!("Burn amount is greater than asset supply")
                                })?
                            }
                            None => {
                                if delta_amount < 0 {
                                    return Err(anyhow::anyhow!("First detected tx is a burn"));
                                }
                                delta_amount as u64
                            }
                        };

                        supply.insert(asset_id, new_amt);
                    }

                    // update info if enabled
                    if let Some(info_map) = new_info.as_mut() {
                        info_map
                            .entry(asset_id)
                            .and_modify(|rec| rec.mint_or_burn_count += 1)
                            .or_insert(AssetInfoRecord {
                                initial_mint_tx_hash: tx_hash.clone(),
                                mint_or_burn_count: 1,
                                onchain_metadata: None,
                                metadata_standard: None,
                            });
                    }

                    // update history if enabled
                    if let Some(hist_map) = new_history.as_mut() {
                        hist_map.entry(asset_id).or_insert_with(Vector::new).push_back(
                            AssetMintRecord {
                                tx_hash: tx_hash.clone(),
                                amount: delta.amount.unsigned_abs(),
                                burn: delta.amount < 0,
                            },
                        );
                    }

                    // update policy index if enabled
                    if let Some(index) = new_index.as_mut() {
                        let ids = index.entry(*policy_id).or_insert_with(Vector::new);
                        if !ids.contains(&asset_id) {
                            ids.push_back(asset_id);
                        }
                    }

                    // initialize addresses if enabled
                    if let Some(addr_map) = new_addresses.as_mut() {
                        addr_map.entry(asset_id).or_insert_with(Vector::new);
                    }

                    // initialize transactions if enabled
                    if let Some(tx_map) = new_transactions.as_mut() {
                        tx_map.entry(asset_id).or_insert_with(Vector::new);
                    }
                }
            }
        }

        Ok(Self {
            config: self.config.clone(),
            supply: new_supply,
            history: new_history,
            info: new_info,
            addresses: new_addresses,
            transactions: new_transactions,
            policy_index: new_index,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        asset_registry::{AssetId, AssetRegistry},
        state::{AssetsStorageConfig, State},
    };
    use acropolis_common::{AssetName, NativeAssetDelta, PolicyId, TxHash};

    fn dummy_policy(byte: u8) -> PolicyId {
        [byte; 28]
    }

    fn asset_name_from_str(s: &str) -> AssetName {
        AssetName::new(s.as_bytes()).unwrap()
    }

    fn dummy_txhash(byte: u8) -> TxHash {
        [byte; 32]
    }

    fn full_config() -> AssetsStorageConfig {
        AssetsStorageConfig {
            store_assets: true,
            store_info: true,
            store_history: true,
            store_transactions: true,
            store_addresses: true,
            index_by_policy: true,
        }
    }

    #[test]
    fn mint_creates_new_asset_and_updates_all_fields() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx = dummy_txhash(9);

        let deltas = vec![(
            tx.clone(),
            vec![(
                policy,
                vec![NativeAssetDelta {
                    name: name.clone(),
                    amount: 100,
                }],
            )],
        )];

        let new_state = state.handle_mint_deltas(&deltas, &mut registry).unwrap();

        // supply updated
        let asset_id = registry.lookup_id(&policy, &name).unwrap();
        assert_eq!(
            new_state.supply.as_ref().unwrap().get(&asset_id),
            Some(&100)
        );

        // info initialized
        let info = new_state.info.as_ref().unwrap().get(&asset_id).unwrap();
        assert_eq!(info.initial_mint_tx_hash, tx);
        assert_eq!(info.mint_or_burn_count, 1);

        // history contains mint record
        let hist = new_state.get_asset_history(&asset_id).unwrap().unwrap();
        assert_eq!(hist[0].amount, 100);
        assert!(!hist[0].burn);

        // policy index updated
        let pol_assets = new_state.get_policy_assets(&policy, &registry).unwrap().unwrap();
        assert_eq!(pol_assets[0].quantity, 100);
    }

    #[test]
    fn second_mint_increments_supply_and_records_mint() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx1 = dummy_txhash(1);
        let tx2 = dummy_txhash(2);

        let deltas1 = vec![(
            tx1.clone(),
            vec![(
                policy,
                vec![NativeAssetDelta {
                    name: name.clone(),
                    amount: 50,
                }],
            )],
        )];
        let deltas2 = vec![(
            tx2.clone(),
            vec![(
                policy,
                vec![NativeAssetDelta {
                    name: name.clone(),
                    amount: 25,
                }],
            )],
        )];

        let state = state.handle_mint_deltas(&deltas1, &mut registry).unwrap();
        let state = state.handle_mint_deltas(&deltas2, &mut registry).unwrap();

        let asset_id = registry.lookup_id(&policy, &name).unwrap();

        // supply updated
        assert_eq!(state.supply.as_ref().unwrap().get(&asset_id), Some(&75));

        // mint/burn count incremented
        assert_eq!(
            state.info.as_ref().unwrap().get(&asset_id).unwrap().mint_or_burn_count,
            2
        );

        // history contains both mint records
        let hist = state.get_asset_history(&asset_id).unwrap().unwrap();
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn burn_reduces_supply_and_records_burn() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx1 = dummy_txhash(1);
        let tx2 = dummy_txhash(2);

        let mint = vec![(
            tx1.clone(),
            vec![(
                policy,
                vec![NativeAssetDelta {
                    name: name.clone(),
                    amount: 100,
                }],
            )],
        )];
        let burn = vec![(
            tx2.clone(),
            vec![(
                policy,
                vec![NativeAssetDelta {
                    name: name.clone(),
                    amount: -40,
                }],
            )],
        )];

        let state = state.handle_mint_deltas(&mint, &mut registry).unwrap();
        let state = state.handle_mint_deltas(&burn, &mut registry).unwrap();

        let asset_id = registry.lookup_id(&policy, &name).unwrap();

        // supply reduced by burn amount
        assert_eq!(state.supply.as_ref().unwrap().get(&asset_id), Some(&60));

        let hist = state.get_asset_history(&asset_id).unwrap().unwrap();

        // history contains both mint and burn records
        assert_eq!(hist.len(), 2);

        // latest entry in history is the burn record
        assert!(hist[1].burn);

        // correct amount stored for burn record
        assert_eq!(hist[1].amount, 40);
    }

    #[test]
    fn first_tx_as_burn_fails() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx = dummy_txhash(1);

        let deltas = vec![(
            tx.clone(),
            vec![(
                policy,
                vec![NativeAssetDelta {
                    name: name.clone(),
                    amount: -50,
                }],
            )],
        )];

        let result = state.handle_mint_deltas(&deltas, &mut registry);
        // Error on first tx being a burn
        assert!(result.is_err());
    }

    #[test]
    fn burn_more_than_supply_fails() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx = dummy_txhash(1);

        let deltas = vec![(
            tx.clone(),
            vec![(
                policy,
                vec![NativeAssetDelta {
                    name: name.clone(),
                    amount: -10,
                }],
            )],
        )];

        let result = state.handle_mint_deltas(&deltas, &mut registry);

        // Error on negative supply
        assert!(result.is_err());
    }

    #[test]
    fn getters_return_error_when_disabled() {
        let config = AssetsStorageConfig::default();
        let state = State::new(config);
        let fake_id = AssetId::new(0);

        // Error when storage is disabled by config
        assert!(state.get_assets_list(&AssetRegistry::new()).is_err());
        assert!(state.get_asset_info(&fake_id).is_err());
        assert!(state.get_asset_history(&fake_id).is_err());
        assert!(state.get_asset_addresses(&fake_id).is_err());
        assert!(state.get_asset_transactions(&fake_id).is_err());
        assert!(state.get_policy_assets(&dummy_policy(1), &AssetRegistry::new()).is_err());
    }
}
