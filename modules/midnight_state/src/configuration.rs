use acropolis_common::{Address, AssetName, PolicyId};
use anyhow::Result;
use config::Config;

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct MidnightConfig {
    // CNight Token
    pub cnight_policy_id: PolicyId,
    pub cnight_asset_name: AssetName,

    // Candidate config
    pub mapping_validator_address: Address,
    pub auth_token_asset_name: AssetName,

    // Governance config
    pub technical_committee_address: Address,
    pub technical_committee_policy_id: PolicyId,
    pub council_address: Address,
    pub council_policy_id: PolicyId,

    // Parameters config
    pub permissioned_candidate_policy: PolicyId,
}

impl MidnightConfig {
    pub fn try_load(config: &Config) -> Result<Self> {
        let full_config = Config::builder().add_source(config.clone()).build()?;
        Ok(full_config.try_deserialize()?)
    }
}
