use crate::{AssetName, PolicyId};

pub const DEFAULT_ASSETS_QUERY_TOPIC: (&str, &str) =
    ("assets-state-query-topic", "cardano.query.assets");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssetsStateQuery {
    GetAssetsList,
    GetAssetInfo { asset_key: Vec<u8> },
    GetAssetHistory { asset_key: Vec<u8> },
    GetAssetTransactions { asset_key: Vec<u8> },
    GetAssetAddresses { asset_key: Vec<u8> },
    GetPolicyIdAssets { policyid_key: Vec<u8> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssetsStateQueryResponse {
    AssetsList(imbl::HashMap<PolicyId, imbl::HashMap<AssetName, u64>>),
    AssetInfo(AssetInfo),
    AssetHistory(AssetHistory),
    AssetTransactions(AssetTransactions),
    AssetAddresses(AssetAddresses),
    PolicyIdAssets(PolicyIdAssets),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetsList {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetInfo {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetHistory {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetTransactions {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetAddresses {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyIdAssets {}
