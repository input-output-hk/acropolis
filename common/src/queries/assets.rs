use crate::{AssetName, PolicyId, TxHash};

pub const DEFAULT_ASSETS_QUERY_TOPIC: (&str, &str) =
    ("assets-state-query-topic", "cardano.query.assets");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssetsStateQuery {
    GetAssetsList,
    GetAssetInfo {
        policy_id: PolicyId,
        asset_name: AssetName,
    },
    GetAssetHistory {
        policy_id: PolicyId,
        asset_name: AssetName,
    },
    GetPolicyIdAssets {
        policy_id: PolicyId,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssetsStateQueryResponse {
    AssetsList(imbl::HashMap<PolicyId, imbl::HashMap<AssetName, u64>>),
    AssetInfo((u64, AssetInfoRecord)),
    AssetHistory(Vec<MintRecord>),
    PolicyIdAssets(Vec<(AssetName, u64)>),
    NotFound,
    Error(String),
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct MintRecord {
    pub tx_hash: TxHash,
    pub amount: u64,
    pub burn: bool,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetInfoRecord {
    pub initial_mint_tx_hash: TxHash,
    pub mint_or_burn_count: u64,
    pub onchain_metadata: bool,
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
