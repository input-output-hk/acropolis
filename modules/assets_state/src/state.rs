//! Acropolis AssetsState: State storage

use acropolis_common::{AssetName, NativeAssetsDelta, PolicyId};
use anyhow::Result;
use imbl::HashMap;
use tracing::info;

#[derive(Debug, Default, Clone)]
pub struct AssetsStorageConfig {
    pub _store_info: bool,
    pub _store_history: bool,
    pub _store_transactions: bool,
    pub _store_addresses: bool,
}

#[derive(Debug, Default, Clone)]
pub struct State {
    pub _config: AssetsStorageConfig,
    pub assets: HashMap<PolicyId, HashMap<AssetName, u64>>,
}

impl State {
    pub fn new(config: &AssetsStorageConfig) -> Self {
        Self {
            _config: config.clone(),
            assets: HashMap::new(),
        }
    }

    pub async fn tick(&self) -> Result<()> {
        let policy_count = self.assets.len();
        let asset_count: usize = self.assets.values().map(|inner| inner.len()).sum();
        info!(
            asset_count,
            policy_count, "Tracking {policy_count} policy ids containing {asset_count} assets"
        );
        Ok(())
    }

    pub fn handle_deltas(&self, deltas: &NativeAssetsDelta) -> Result<Self> {
        let mut new_assets = self.assets.clone();

        for (policy_id, asset_deltas) in deltas {
            let mut policy_entry = new_assets.get(policy_id).cloned().unwrap_or_default();

            for delta in asset_deltas {
                let current = policy_entry.get(&delta.name).cloned().unwrap_or(0);
                let sum = (current as i128) + (delta.amount as i128);

                let new_amt = u64::try_from(sum)
                    .map_err(|_| anyhow::anyhow!("More asset burned than supply"))?;

                if new_amt == 0 {
                    policy_entry.remove(&delta.name);
                } else {
                    policy_entry.insert(delta.name.clone(), new_amt);
                }
            }

            new_assets.insert(*policy_id, policy_entry);
        }

        Ok(Self {
            _config: self._config.clone(),
            assets: new_assets,
        })
    }
}
