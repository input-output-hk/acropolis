//! Acropolis AssetsState: State storage

use crate::asset_registry::{AssetId, AssetRegistry};
use acropolis_common::{
    queries::assets::{
        AssetHistory, AssetInfoRecord, AssetListEntry, AssetMetadataStandard, MintRecord,
        PolicyAsset, PolicyAssets,
    },
    AssetName, Datum, NativeAssetDelta, PolicyId, ShelleyAddress, TxHash, UTXODelta,
};
use anyhow::Result;
use imbl::{HashMap, Vector};
use tracing::{error, info};

#[derive(Debug, Default, Clone, Copy)]
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

    pub fn get_asset_info(
        &self,
        asset_id: &AssetId,
        registry: &AssetRegistry,
    ) -> Result<Option<(u64, AssetInfoRecord)>> {
        if !self.config.store_info {
            return Err(anyhow::anyhow!("asset info storage disabled in config"));
        }

        let supply = self.supply.as_ref().and_then(|supply_map| supply_map.get(asset_id));
        let mut info = self.info.as_ref().and_then(|info_map| info_map.get(asset_id)).cloned();

        // Overwrite asset metadata if an associated CIP68 reference token is found
        if let Some(ref_info) = self.resolve_cip68_metadata(asset_id, registry) {
            if let Some(info_mut) = info.as_mut() {
                info_mut.onchain_metadata = ref_info.onchain_metadata;
                info_mut.metadata_standard = ref_info.metadata_standard;
            } else {
                info = Some(ref_info);
            }
        }

        Ok(match (supply, info) {
            (Some(supply), Some(info)) => Some((*supply, info)),
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

    // TODO: Allow tick to log based on any enabled field instead of only supply
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
                info!("Tracking {asset_count} assets");
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

                        supply
                            .entry(asset_id)
                            .and_modify(|current| {
                                let sum = (*current as i128) + (delta_amount as i128);
                                match u64::try_from(sum) {
                                    Ok(new_amt) => *current = new_amt,
                                    Err(_) => {
                                        error!("Burn amount is greater than asset supply");
                                    }
                                }
                            })
                            .or_insert_with(|| {
                                if delta_amount < 0 {
                                    error!("First detected tx is a burn");
                                    0
                                } else {
                                    delta_amount as u64
                                }
                            });
                    }

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

                    if let Some(hist_map) = new_history.as_mut() {
                        hist_map.entry(asset_id).or_insert_with(Vector::new).push_back(
                            AssetMintRecord {
                                tx_hash: tx_hash.clone(),
                                amount: delta.amount.unsigned_abs(),
                                burn: delta.amount < 0,
                            },
                        );
                    }

                    if let Some(index) = new_index.as_mut() {
                        let ids = index.entry(*policy_id).or_insert_with(Vector::new);
                        if !ids.contains(&asset_id) {
                            ids.push_back(asset_id);
                        }
                    }
                    if let Some(addr_map) = new_addresses.as_mut() {
                        addr_map.entry(asset_id).or_insert_with(Vector::new);
                    }
                    if let Some(tx_map) = new_transactions.as_mut() {
                        tx_map.entry(asset_id).or_insert_with(Vector::new);
                    }
                }
            }
        }

        Ok(Self {
            config: self.config,
            supply: new_supply,
            history: new_history,
            info: new_info,
            addresses: new_addresses,
            transactions: new_transactions,
            policy_index: new_index,
        })
    }

    // TODO: Potentially store metadata for assets that have not been minted yet (pre-mint metadata)
    pub fn handle_cip25_metadata(
        &self,
        registry: &mut AssetRegistry,
        metadata_bytes: &[Vec<u8>],
    ) -> Result<Self> {
        let mut new_info = self.info.clone();
        let Some(info_map) = new_info.as_mut() else {
            return Ok(Self {
                config: self.config,
                supply: self.supply.clone(),
                history: self.history.clone(),
                info: new_info,
                addresses: self.addresses.clone(),
                transactions: self.transactions.clone(),
                policy_index: self.policy_index.clone(),
            });
        };

        for bytes in metadata_bytes {
            let Ok(decoded) = serde_cbor::from_slice::<serde_cbor::Value>(bytes) else {
                continue;
            };
            let serde_cbor::Value::Map(mut policy_map) = decoded else {
                continue;
            };

            // Retrieve CIP25 version from map and default to v1 if missing
            let version_key = serde_cbor::Value::Text("version".to_string());
            let mut standard = AssetMetadataStandard::CIP25v1;
            if let Some(serde_cbor::Value::Text(ver)) = policy_map.get(&version_key) {
                if ver == "2.0" {
                    standard = AssetMetadataStandard::CIP25v2;
                }
                policy_map.remove(&version_key);
            }

            for (policy_key, assets_val) in policy_map {
                let (serde_cbor::Value::Text(policy_hex), serde_cbor::Value::Map(asset_map)) =
                    (policy_key, assets_val)
                else {
                    continue;
                };

                let Some(policy_id) = hex::decode(policy_hex).ok().and_then(|b| b.try_into().ok())
                else {
                    continue;
                };

                for (asset_key, metadata_val) in asset_map {
                    if let serde_cbor::Value::Text(asset_hex) = asset_key {
                        if let Ok(asset_bytes) = hex::decode(&asset_hex) {
                            if let Some(asset_name) = AssetName::new(&asset_bytes) {
                                if let Some(asset_id) = registry.lookup_id(&policy_id, &asset_name)
                                {
                                    if let Ok(metadata_raw) = serde_cbor::to_vec(&metadata_val) {
                                        if let Some(record) = info_map.get_mut(&asset_id) {
                                            record.onchain_metadata = Some(metadata_raw);
                                            record.metadata_standard = Some(standard);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            config: self.config.clone(),
            supply: self.supply.clone(),
            history: self.history.clone(),
            info: new_info,
            addresses: self.addresses.clone(),
            transactions: self.transactions.clone(),
            policy_index: self.policy_index.clone(),
        })
    }

    // TODO: Potentially store metadata for user tokens that have not been minted yet (pre-mint metadata)
    pub fn handle_cip68_metadata(
        &self,
        deltas: &[UTXODelta],
        registry: &mut AssetRegistry,
    ) -> Result<Self> {
        let mut new_info = self.info.clone();

        for delta in deltas {
            let UTXODelta::Output(output) = delta else {
                continue;
            };
            let Some(Datum::Inline(blob)) = &output.datum else {
                continue;
            };

            const CIP68_REFERENCE_PREFIX: [u8; 4] = [0x00, 0x06, 0x43, 0xb0];
            for (policy_id, native_assets) in &output.value.assets {
                for asset in native_assets {
                    let name = &asset.name;

                    if !name.as_slice().starts_with(&CIP68_REFERENCE_PREFIX) {
                        continue;
                    }

                    // NOTE: CIP68 metadata version is included in the blob and is decoded in REST handler
                    match registry.lookup_id(policy_id, name) {
                        Some(asset_id) => {
                            if let Some(record) =
                                new_info.as_mut().and_then(|m| m.get_mut(&asset_id))
                            {
                                record.onchain_metadata = Some(blob.clone());
                            }
                        }
                        None => {
                            error!(
                                "Got CIP-68 datum for unknown asset: {}.{}",
                                hex::encode(policy_id),
                                hex::encode(name.as_slice())
                            );
                        }
                    }
                }
            }
        }

        Ok(Self {
            config: self.config,
            supply: self.supply.clone(),
            history: self.history.clone(),
            info: new_info,
            addresses: self.addresses.clone(),
            transactions: self.transactions.clone(),
            policy_index: self.policy_index.clone(),
        })
    }

    fn resolve_cip68_metadata(
        &self,
        asset_id: &AssetId,
        registry: &AssetRegistry,
    ) -> Option<AssetInfoRecord> {
        let key = registry.lookup(*asset_id)?;
        let name_bytes = key.name.as_slice();
        let label = u32::from_be_bytes(name_bytes.get(0..4)?.try_into().ok()?);

        match label {
            // Reference NFT (100) label
            0x000643b0 => self.info.as_ref()?.get(asset_id).cloned().map(|mut rec| {
                // Hide metadata on the reference itself (Per Blockfrost spec)
                rec.onchain_metadata = None;
                rec.metadata_standard = None;
                rec
            }),

            // CIP-67 prefixes for user token labels (222, 333, 444)
            0x000de140 | 0x0014df10 | 0x001b4e20 => {
                let mut ref_bytes = name_bytes.to_vec();
                ref_bytes[0..4].copy_from_slice(&[0x00, 0x06, 0x43, 0xb0]);
                let ref_name = AssetName::new(&ref_bytes)?;
                let ref_id = registry.lookup_id(&key.policy, &ref_name)?;
                self.info.as_ref()?.get(&ref_id).cloned()
            }

            _ => None,
        }
    }
}
