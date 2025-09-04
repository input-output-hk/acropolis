//! Acropolis AssetsState: State storage

use acropolis_common::{AssetName, NativeAssetsDelta, PolicyId};
use anyhow::Result;
use std::collections::HashMap;
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

    pub fn handle_deltas(&mut self, deltas: &NativeAssetsDelta) -> Result<()> {
        for (policy_id, asset_deltas) in deltas {
            let policy_entry = self.assets.entry(*policy_id).or_default();

            for delta in asset_deltas {
                let current = policy_entry.entry(delta.name.clone()).or_insert(0);
                let new_amt = u64::try_from((*current as i128) + delta.amount as i128)
                    .map_err(|_| anyhow::anyhow!("More asset burned than supply"))?;
                if new_amt == 0 {
                    policy_entry.remove(&delta.name);
                } else {
                    *current = new_amt as u64;
                }
            }
        }
        Ok(())
    }
}
