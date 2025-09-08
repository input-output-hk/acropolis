//! Acropolis AssetsState: State storage

use acropolis_common::{
    queries::assets::{
        AssetAddresses, AssetHistory, AssetInfo, AssetInfoRecord, AssetList, AssetTransactions,
        MintRecord,
    },
    AssetName, NativeAssetsDelta, PolicyId, TxHash,
};
use anyhow::Result;
use imbl::HashMap;
use tracing::info;

#[derive(Debug, Default, Clone)]
pub struct AssetsStorageConfig {
    pub store_info: bool,
    pub store_history: bool,
    pub store_addresses: bool,
    pub store_transactions: bool,
}

#[derive(Debug, Default, Clone)]
pub struct AssetRecord {
    pub supply: u64,
    pub history: Option<AssetHistory>,
    pub extended_info: Option<AssetInfoRecord>,
    pub addresses: Option<AssetAddresses>,
    pub transactions: Option<AssetTransactions>,
}

#[derive(Debug, Default, Clone)]
pub struct State {
    pub config: AssetsStorageConfig,
    pub assets: HashMap<PolicyId, HashMap<AssetName, AssetRecord>>,
}

impl State {
    pub fn new(cfg: &AssetsStorageConfig) -> Self {
        Self {
            config: cfg.clone(),
            assets: HashMap::new(),
        }
    }

    pub fn get_asset_list(&self) -> AssetList {
        let mut result = Vec::new();

        for (policy_id, assets) in &self.assets {
            for (asset_name, record) in assets {
                result.push((*policy_id, asset_name.clone(), record.supply));
            }
        }

        result
    }

    pub fn get_asset_info(
        &self,
        policy_id: &PolicyId,
        asset_name: &AssetName,
    ) -> Result<Option<AssetInfo>> {
        if !self.config.store_info {
            return Err(anyhow::anyhow!("asset info storage disabled in config"));
        }

        Ok(self
            .assets
            .get(policy_id)
            .and_then(|policy_entry| policy_entry.get(asset_name))
            .and_then(|asset_entry| {
                asset_entry.extended_info.clone().map(|info| (asset_entry.supply, info))
            }))
    }

    pub fn get_asset_history(
        &self,
        policy_id: &PolicyId,
        asset_name: &AssetName,
    ) -> Result<Option<AssetHistory>> {
        if !self.config.store_history {
            return Err(anyhow::anyhow!("asset history storage disabled in config"));
        }

        Ok(self
            .assets
            .get(policy_id)
            .and_then(|policy_entry| policy_entry.get(asset_name))
            .and_then(|asset_entry| asset_entry.history.clone()))
    }

    pub fn get_asset_addresses(
        &self,
        policy_id: &PolicyId,
        asset_name: &AssetName,
    ) -> Result<Option<AssetAddresses>> {
        if !self.config.store_addresses {
            return Err(anyhow::anyhow!(
                "asset addresses storage disabled in config"
            ));
        }

        Ok(self
            .assets
            .get(policy_id)
            .and_then(|policy_entry| policy_entry.get(asset_name))
            .and_then(|asset_entry| asset_entry.addresses.clone()))
    }

    pub fn get_asset_transactions(
        &self,
        policy_id: &PolicyId,
        asset_name: &AssetName,
    ) -> Result<Option<AssetTransactions>> {
        if !self.config.store_transactions {
            return Err(anyhow::anyhow!(
                "asset transactions storage disabled in config"
            ));
        }

        Ok(self
            .assets
            .get(policy_id)
            .and_then(|policy_entry| policy_entry.get(asset_name))
            .and_then(|asset_entry| asset_entry.transactions.clone()))
    }

    pub fn get_policy_assets(&self, policy_id: &PolicyId) -> Option<Vec<(AssetName, u64)>> {
        self.assets.get(policy_id).map(|assets| {
            assets.iter().map(|(asset_name, record)| (asset_name.clone(), record.supply)).collect()
        })
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

    pub fn handle_deltas(&self, tx_hash: &TxHash, deltas: &NativeAssetsDelta) -> Result<Self> {
        let mut new_assets = self.assets.clone();

        for (policy_id, asset_deltas) in deltas {
            let mut policy_entry = new_assets.get(policy_id).cloned().unwrap_or_default();

            for delta in asset_deltas {
                let (amount, burn) = if delta.amount < 0 {
                    ((-delta.amount) as u64, true)
                } else {
                    (delta.amount as u64, false)
                };

                // Get or initialize asset record
                let mut record =
                    policy_entry.get(&delta.name).cloned().unwrap_or_else(|| AssetRecord {
                        supply: 0,
                        history: if self.config.store_history {
                            Some(Vec::new())
                        } else {
                            None
                        },
                        extended_info: if self.config.store_info {
                            Some(AssetInfoRecord {
                                initial_mint_tx_hash: tx_hash.clone(),
                                mint_or_burn_count: 0,
                                onchain_metadata: None,
                                metadata_standard: None,
                                metadata_extra: None,
                            })
                        } else {
                            None
                        },
                        addresses: if self.config.store_addresses {
                            Some(Vec::new())
                        } else {
                            None
                        },
                        transactions: if self.config.store_transactions {
                            Some(Vec::new())
                        } else {
                            None
                        },
                    });

                // Update supply
                let sum = (record.supply as i128) + (delta.amount as i128);
                record.supply = u64::try_from(sum)
                    .map_err(|_| anyhow::anyhow!("More asset burned than supply"))?;

                // Append to history if enabled
                if let Some(history) = record.history.as_mut() {
                    history.push(MintRecord {
                        tx_hash: tx_hash.clone(),
                        amount,
                        burn,
                    });
                }

                // Update extended info if enabled
                if let Some(info) = record.extended_info.as_mut() {
                    info.mint_or_burn_count += 1;
                }

                policy_entry.insert(delta.name.clone(), record);
            }

            new_assets.insert(*policy_id, policy_entry);
        }

        Ok(Self {
            config: self.config.clone(),
            assets: new_assets,
        })
    }
}
