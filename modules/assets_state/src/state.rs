//! Acropolis AssetsState: State storage

use crate::asset_registry::{AssetId, AssetRegistry};
use acropolis_common::{AssetName, NativeAssetsDelta, PolicyId, ShelleyAddress, TxHash};
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetInfoRecord {
    pub metadata: Vec<u8>,
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
    pub addresses: Option<HashMap<AssetId, Vector<ShelleyAddress>>>,

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

    pub fn get_assets_list(
        &self,
        registry: &AssetRegistry,
    ) -> Result<Vec<(PolicyId, AssetName, u64)>, &'static str> {
        let supply = self.supply.as_ref().ok_or("Asset storage is disabled by configuration.")?;

        let mut out = Vec::with_capacity(supply.len());
        for (id, amount) in supply {
            if let Some(key) = registry.lookup(*id) {
                out.push((*key.policy.as_ref(), (*key.name.as_ref()).clone(), *amount));
            }
        }

        Ok(out)
    }

    pub async fn tick(&self) -> Result<()> {
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

    pub fn handle_deltas(
        &self,
        deltas: &NativeAssetsDelta,
        registry: &mut AssetRegistry,
    ) -> Result<Self> {
        let mut new_supply = self.supply.clone();
        let mut new_index = self.policy_index.clone();

        if let Some(supply) = new_supply.as_mut() {
            for (policy_id, asset_deltas) in deltas {
                for delta in asset_deltas {
                    let asset_id = registry.get_or_insert(*policy_id, delta.name.clone());

                    let current = supply.get(&asset_id).copied().unwrap_or(0);
                    let sum = (current as i128) + (delta.amount as i128);

                    let new_amt = u64::try_from(sum)
                        .map_err(|_| anyhow::anyhow!("More asset burned than supply"))?;

                    let existed = supply.insert(asset_id, new_amt).is_some();

                    if !existed {
                        if let Some(index) = new_index.as_mut() {
                            index.entry(*policy_id).or_insert_with(Vector::new).push_back(asset_id);
                        }
                    }
                }
            }
        }

        Ok(Self {
            config: self.config.clone(),
            supply: new_supply,
            history: self.history.clone(),
            info: self.info.clone(),
            addresses: self.addresses.clone(),
            transactions: self.transactions.clone(),
            policy_index: new_index,
        })
    }
}
