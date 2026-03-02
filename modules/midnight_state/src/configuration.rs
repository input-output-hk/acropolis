use std::net::SocketAddr;

use acropolis_common::{Address, AssetName, PolicyId};
use anyhow::{anyhow, Result};
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
    #[serde(skip)] // Derived from `mapping_validator_address`
    pub auth_token_policy_id: PolicyId,

    // Governance config
    pub technical_committee_address: Address,
    pub technical_committee_policy_id: PolicyId,
    pub council_address: Address,
    pub council_policy_id: PolicyId,

    // Parameters config
    pub permissioned_candidate_policy: PolicyId,

    // gRPC config
    pub grpc_bind_address: String,
}

impl MidnightConfig {
    pub fn try_load(config: &Config) -> Result<Self> {
        let full_config = Config::builder().add_source(config.clone()).build()?;
        let mut cfg: MidnightConfig = full_config.try_deserialize()?;
        // Derive the candidate auth token based on the validator address
        cfg.auth_token_policy_id = PolicyId::from(
            cfg.mapping_validator_address
                .get_payment_part()
                .ok_or_else(|| anyhow!("address is not a Shelley address"))?
                .to_script_hash()
                .ok_or_else(|| anyhow!("address is not a script address"))?,
        );
        Ok(cfg)
    }

    pub fn grpc_socket_addr(&self) -> Result<SocketAddr> {
        self.grpc_bind_address.parse().map_err(|e| {
            anyhow!(
                "invalid grpc_bind_address '{}': {e}",
                self.grpc_bind_address
            )
        })
    }
}
