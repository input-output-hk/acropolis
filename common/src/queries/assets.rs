use crate::{AssetName, PolicyId, ShelleyAddress, TxHash};

pub const DEFAULT_ASSETS_QUERY_TOPIC: (&str, &str) =
    ("assets-state-query-topic", "cardano.query.assets");

pub type AssetList = imbl::HashMap<PolicyId, imbl::HashMap<AssetName, u64>>;
pub type AssetInfo = (u64, AssetInfoRecord);
pub type AssetHistory = Vec<MintRecord>;
pub type PolicyIdAssets = Vec<(AssetName, u64)>;
pub type AssetAddresses = Vec<(ShelleyAddress, u64)>;
pub type AssetTransactions = Vec<TxHash>;

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
    GetAssetAddresses {
        policy_id: PolicyId,
        asset_name: AssetName,
    },
    GetAssetTransactions {
        policy_id: PolicyId,
        asset_name: AssetName,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssetsStateQueryResponse {
    AssetsList(AssetList),
    AssetInfo(AssetInfo),
    AssetHistory(AssetHistory),
    PolicyIdAssets(PolicyIdAssets),
    AssetAddresses(AssetAddresses),
    AssetTransactions(AssetTransactions),
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
