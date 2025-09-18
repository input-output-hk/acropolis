use crate::{
    AssetAddressEntry, AssetInfoRecord, AssetMintRecord, AssetName, PolicyAsset, PolicyId, TxHash,
};

pub const DEFAULT_ASSETS_QUERY_TOPIC: (&str, &str) =
    ("assets-state-query-topic", "cardano.query.assets");

pub const DEFAULT_OFFCHAIN_TOKEN_REGISTRY_URL: (&str, &str) = (
    "offchain-token-registry-url",
    "https://raw.githubusercontent.com/cardano-foundation/cardano-token-registry/master/mappings/",
);

pub type AssetList = Vec<PolicyAsset>;
pub type AssetInfo = (u64, AssetInfoRecord);
pub type AssetHistory = Vec<AssetMintRecord>;
pub type AssetAddresses = Vec<AssetAddressEntry>;
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
