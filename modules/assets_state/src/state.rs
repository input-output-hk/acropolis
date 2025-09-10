//! Acropolis AssetsState: State storage

use crate::asset_registry::{AssetId, AssetRegistry};
use acropolis_common::{
    queries::assets::{
        AssetHistory, AssetInfoRecord, AssetListEntry, MintRecord, PolicyAsset, PolicyAssets,
    },
    NativeAssetDelta, PolicyId, ShelleyAddress, TxHash,
};
use anyhow::Result;
use imbl::{HashMap, Vector};
use tracing::info;

#[derive(Debug, Default, Clone)]
pub struct AssetsStorageConfig {
    pub store_assets: bool,
    pub store_info: bool,
    pub store_history: bool,
    pub store_transactions: bool,
    pub store_addresses: bool,
    pub index_by_policy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetMintRecord {
    pub tx_hash: TxHash,
    pub amount: u64,
    pub burn: bool,
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

    pub fn get_assets_list(&self, registry: &AssetRegistry) -> Result<Vec<AssetListEntry>> {
        let supply = self
            .supply
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("asset storage is disabled in config"))?;

        let mut out = Vec::with_capacity(supply.len());
        for (id, amount) in supply {
            if let Some(key) = registry.lookup(*id) {
                out.push(AssetListEntry {
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

        let maybe_vec =
            self.history.as_ref().and_then(|hist_map| hist_map.get(asset_id)).map(|v| {
                v.iter()
                    .map(|rec| MintRecord {
                        tx_hash: rec.tx_hash.clone(),
                        amount: rec.amount,
                        burn: rec.burn,
                    })
                    .collect::<Vec<MintRecord>>()
            });

        Ok(maybe_vec)
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

        if let Some(supply) = new_supply.as_mut() {
            for (tx_hash, tx_deltas) in deltas {
                for (policy_id, asset_deltas) in tx_deltas {
                    for delta in asset_deltas {
                        let asset_id = registry.get_or_insert(*policy_id, delta.name.clone());

                        // update supply
                        let current = supply.get(&asset_id).copied().unwrap_or(0);
                        let sum = (current as i128) + (delta.amount as i128);

                        let new_amt = u64::try_from(sum)
                            .map_err(|_| anyhow::anyhow!("More asset burned than supply"))?;

                        let existed = supply.insert(asset_id, new_amt).is_some();

                        // update info if enabled
                        if let Some(info_map) = new_info.as_mut() {
                            if !existed {
                                info_map.insert(
                                    asset_id,
                                    AssetInfoRecord {
                                        initial_mint_tx_hash: tx_hash.clone(),
                                        mint_or_burn_count: 1,
                                        onchain_metadata: None,
                                        metadata_standard: None,
                                        metadata_extra: None,
                                    },
                                );
                            } else if let Some(info) = info_map.get_mut(&asset_id) {
                                info.mint_or_burn_count += 1;
                            }
                        }

                        // update policy index if enabled
                        if !existed {
                            if let Some(index) = new_index.as_mut() {
                                index
                                    .entry(*policy_id)
                                    .or_insert_with(Vector::new)
                                    .push_back(asset_id);
                            }
                        }

                        // initialize addresses if enabled
                        if !existed {
                            if let Some(addr_map) = new_addresses.as_mut() {
                                addr_map.insert(asset_id, Vector::new());
                            }
                        }

                        // initialize transactions if enabled
                        if !existed {
                            if let Some(tx_map) = new_transactions.as_mut() {
                                tx_map.insert(asset_id, Vector::new());
                            }
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
