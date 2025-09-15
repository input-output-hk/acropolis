use crate::{AssetName, PolicyId, ShelleyAddress, TxHash};

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

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetInfoRecord {
    pub initial_mint_tx_hash: TxHash,
    pub mint_or_burn_count: u64,
    pub onchain_metadata: Option<Vec<u8>>,
    pub metadata_standard: Option<AssetMetadataStandard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AssetMintRecord {
    pub tx_hash: TxHash,
    pub amount: u64,
    pub burn: bool,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum AssetMetadataStandard {
    CIP25v1,
    CIP25v2,
    CIP68v1,
    CIP68v2,
    CIP68v3,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyAsset {
    pub policy: PolicyId,
    pub name: AssetName,
    pub quantity: u64,
}
