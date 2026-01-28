use std::{
    collections::HashMap,
    ops::{Add, AddAssign, Neg},
};

use crate::hash::Hash;

pub type PolicyId = Hash<28>;
pub type NativeAssets = Vec<(PolicyId, Vec<NativeAsset>)>;
pub type NativeAssetsDelta = Vec<(PolicyId, Vec<NativeAssetDelta>)>;
pub type NativeAssetsMap = HashMap<PolicyId, HashMap<AssetName, u64>>;
pub type NativeAssetsDeltaMap = HashMap<PolicyId, HashMap<AssetName, i64>>;

#[derive(
    Debug,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct AssetName {
    #[n(0)]
    len: u8,
    #[n(1)]
    bytes: [u8; 32],
}

impl AssetName {
    pub fn new(data: &[u8]) -> Option<Self> {
        if data.len() > 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes[..data.len()].copy_from_slice(data);
        Some(Self {
            len: data.len() as u8,
            bytes,
        })
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct NativeAsset {
    #[n(0)]
    pub name: AssetName,
    #[n(1)]
    pub amount: u64,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, minicbor::Encode, minicbor::Decode,
)]
pub struct NativeAssetDelta {
    #[n(0)]
    pub name: AssetName,
    #[n(1)]
    pub amount: i64,
}

/// Value (lovelace + multiasset)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Value {
    pub lovelace: u64,
    pub assets: NativeAssets,
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if self.lovelace != other.lovelace {
            return false;
        }

        if self.assets.len() != other.assets.len() {
            return false;
        }
        let mut counts: HashMap<Vec<u8>, i64> = HashMap::new();
        for (policy_id, assets) in &self.assets {
            for asset in assets {
                let mut asset_id = policy_id.to_vec();
                asset_id.extend_from_slice(asset.name.as_slice());
                *counts.entry(asset_id).or_default() += asset.amount as i64;
            }
        }
        for (policy_id, assets) in &other.assets {
            for asset in assets {
                let mut asset_id = policy_id.to_vec();
                asset_id.extend_from_slice(asset.name.as_slice());
                let count = counts.entry(asset_id).or_default();
                *count = count.saturating_sub(asset.amount as i64);
                if *count != 0 {
                    return false;
                }
            }
        }
        true
    }
}

impl Eq for Value {}

impl Value {
    pub fn new(lovelace: u64, assets: NativeAssets) -> Self {
        Self { lovelace, assets }
    }

    pub fn coin(&self) -> u64 {
        self.lovelace
    }

    pub fn sum_lovelace<'a>(iter: impl Iterator<Item = &'a Value>) -> u64 {
        iter.map(|v| v.lovelace).sum()
    }
}

impl AddAssign<&Value> for Value {
    fn add_assign(&mut self, other: &Value) {
        self.lovelace += other.lovelace;

        for (policy_id, other_assets) in &other.assets {
            if let Some((_, existing_assets)) =
                self.assets.iter_mut().find(|(pid, _)| pid == policy_id)
            {
                for other_asset in other_assets {
                    if let Some(existing) =
                        existing_assets.iter_mut().find(|a| a.name == other_asset.name)
                    {
                        existing.amount += other_asset.amount;
                    } else {
                        existing_assets.push(other_asset.clone());
                    }
                }
            } else {
                self.assets.push((*policy_id, other_assets.clone()));
            }
        }
    }
}

impl Add for Value {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        let mut result = self.clone();
        result += &other;
        result
    }
}

/// Hashmap representation of Value (lovelace + multiasset)
#[derive(
    Debug, Default, Clone, serde::Serialize, serde::Deserialize, minicbor::Encode, minicbor::Decode,
)]
pub struct ValueMap {
    #[n(0)]
    pub lovelace: u64,
    #[n(1)]
    pub assets: NativeAssetsMap,
}

impl AddAssign for ValueMap {
    fn add_assign(&mut self, other: Self) {
        self.lovelace += other.lovelace;

        for (policy, assets) in other.assets {
            let entry = self.assets.entry(policy).or_default();
            for (asset_name, amount) in assets {
                *entry.entry(asset_name).or_default() += amount;
            }
        }
    }
}

impl ValueMap {
    pub fn add_value(&mut self, other: &Value) {
        // Handle lovelace
        self.lovelace = self.lovelace.saturating_add(other.lovelace);

        // Handle multi-assets
        for (policy, assets) in &other.assets {
            let policy_entry = self.assets.entry(*policy).or_default();
            for asset in assets {
                *policy_entry.entry(asset.name).or_default() = policy_entry
                    .get(&asset.name)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(asset.amount);
            }
        }
    }
}

impl From<ValueMap> for Value {
    fn from(map: ValueMap) -> Self {
        Self {
            lovelace: map.lovelace,
            assets: map
                .assets
                .into_iter()
                .map(|(policy, assets)| {
                    (
                        policy,
                        assets
                            .into_iter()
                            .map(|(name, amount)| NativeAsset { name, amount })
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValueDelta {
    pub lovelace: i64,
    pub assets: NativeAssetsDelta,
}

impl ValueDelta {
    pub fn new(lovelace: i64, assets: NativeAssetsDelta) -> Self {
        Self { lovelace, assets }
    }
}

impl From<ValueMap> for ValueDelta {
    fn from(map: ValueMap) -> Self {
        Self {
            lovelace: map.lovelace as i64,
            assets: map
                .assets
                .into_iter()
                .map(|(policy, assets)| {
                    (
                        policy,
                        assets
                            .into_iter()
                            .map(|(name, amount)| NativeAssetDelta {
                                name,
                                amount: amount as i64,
                            })
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}

impl From<&Value> for ValueDelta {
    fn from(v: &Value) -> Self {
        ValueDelta {
            lovelace: v.lovelace as i64,
            assets: v
                .assets
                .iter()
                .map(|(pid, nas)| {
                    let nas_delta = nas
                        .iter()
                        .map(|na| NativeAssetDelta {
                            name: na.name,
                            amount: na.amount as i64,
                        })
                        .collect();
                    (*pid, nas_delta)
                })
                .collect(),
        }
    }
}

impl Neg for ValueDelta {
    type Output = Self;

    fn neg(mut self) -> Self::Output {
        self.lovelace = -self.lovelace;
        for (_, nas) in &mut self.assets {
            for na in nas {
                na.amount = -na.amount;
            }
        }
        self
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyAsset {
    pub policy: PolicyId,
    pub name: AssetName,
    pub quantity: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AssetMetadataStandard {
    CIP25v1,
    CIP25v2,
    CIP68v1,
    CIP68v2,
    CIP68v3,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetMetadata {
    pub cip25_metadata: Option<Vec<u8>>,
    pub cip25_version: Option<AssetMetadataStandard>,
    pub cip68_metadata: Option<Vec<u8>>,
    pub cip68_version: Option<AssetMetadataStandard>,
}
