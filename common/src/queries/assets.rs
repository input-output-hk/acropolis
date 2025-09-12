use serde::Serialize;

use crate::{AssetName, PolicyId, ShelleyAddress, TxHash};

pub const DEFAULT_ASSETS_QUERY_TOPIC: (&str, &str) =
    ("assets-state-query-topic", "cardano.query.assets");

pub type AssetList = Vec<PolicyAsset>;
pub type AssetInfo = (u64, AssetInfoRecord);
pub type AssetHistory = Vec<MintRecord>;
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
    pub metadata_extra: Option<Vec<u8>>,
}

#[derive(Debug, Default, Clone, serde::Deserialize)]
pub struct MintRecord {
    pub tx_hash: TxHash,
    pub amount: u64,
    pub burn: bool,
}

impl serde::Serialize for MintRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("tx_hash", &hex::encode(self.tx_hash))?;

        let action = if self.burn { "burned" } else { "minted" };
        map.serialize_entry("action", action)?;

        map.serialize_entry("amount", &self.amount.to_string())?;
        map.end()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssetMetadataStandard {
    CIP25v1,
    CIP25v2,
    CIP68v1,
    CIP68v2,
    CIP68v3,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PolicyAsset {
    pub policy: PolicyId,
    pub name: AssetName,
    pub quantity: u64,
}

impl Serialize for PolicyAsset {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(2))?;
        let asset_hex = format!(
            "{}{}",
            hex::encode(self.policy),
            hex::encode(self.name.as_slice())
        );
        map.serialize_entry("asset", &asset_hex)?;
        map.serialize_entry("quantity", &self.quantity.to_string())?;
        map.end()
    }
}
