use crate::{
    AssetInfoRecord, AssetMintRecord, AssetName, PolicyAsset, PolicyId, ShelleyAddress, TxHash,
};

pub const DEFAULT_ASSETS_QUERY_TOPIC: (&str, &str) =
    ("assets-state-query-topic", "cardano.query.assets");

pub type AssetList = Vec<PolicyAsset>;
pub type AssetInfo = (u64, AssetInfoRecord);
pub type AssetHistory = Vec<AssetMintRecord>;
pub type AssetAddresses = Vec<(ShelleyAddress, u64)>;
pub type AssetTransactions = Vec<TxHash>;
pub type PolicyAssets = Vec<PolicyAsset>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssetsStateQuery {
    GetAssetsList,
    GetAssetInfo { policy: PolicyId, name: AssetName },
    GetAssetHistory { policy: PolicyId, name: AssetName },
    GetPolicyIdAssets { policy: PolicyId },
    GetAssetAddresses { policy: PolicyId, name: AssetName },
    GetAssetTransactions { policy: PolicyId, name: AssetName },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssetsStateQueryResponse {
    AssetsList(AssetList),
    AssetInfo(AssetInfo),
    AssetHistory(AssetHistory),
    AssetAddresses(AssetAddresses),
    AssetTransactions(AssetTransactions),
    PolicyIdAssets(PolicyAssets),
    NotFound,
    Error(String),
}
