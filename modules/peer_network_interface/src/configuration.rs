use std::net::IpAddr;
use std::path::PathBuf;

use acropolis_common::genesis_values::GenesisValues;
use anyhow::Result;
use config::Config;

#[derive(Clone, Debug, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SyncPoint {
    Origin,
    Tip,
    Cache,
    Dynamic,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct InterfaceConfig {
    pub block_topic: String,
    pub sync_point: SyncPoint,
    pub genesis_completion_topic: String,
    pub sync_command_topic: String,
    pub node_addresses: Vec<String>,
    pub cache_dir: PathBuf,
    #[serde(flatten)]
    pub genesis_values: Option<GenesisValues>,
    #[serde(default = "default_consensus_topic")]
    pub consensus_topic: String,
    #[serde(default = "default_block_wanted_topic")]
    pub block_wanted_topic: String,
    #[serde(default = "default_target_peer_count")]
    pub target_peer_count: usize,
    #[serde(default = "default_min_hot_peers")]
    pub min_hot_peers: usize,
    #[serde(default = "default_peer_sharing_enabled")]
    pub peer_sharing_enabled: bool,
    #[serde(default = "default_churn_interval_secs")]
    pub churn_interval_secs: u64,
    #[serde(default = "default_peer_sharing_timeout_secs")]
    pub peer_sharing_timeout_secs: u64,
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_ipv6_enabled")]
    pub ipv6_enabled: bool,
    #[serde(default = "default_allow_non_public_peer_addrs")]
    pub allow_non_public_peer_addrs: bool,
    #[serde(default = "default_discovery_interval_secs")]
    pub discovery_interval_secs: u64,
    #[serde(default = "default_peer_sharing_cooldown_secs")]
    pub peer_sharing_cooldown_secs: u64,
    /// If set, any peer-sharing address matching this IP is rewritten to localhost.
    /// Useful for Docker Desktop, which advertises its host-gateway (e.g. 192.168.65.254)
    /// in peer-sharing responses — an address not routable from the host.
    pub localhost_gateway_ip: Option<IpAddr>,
}

fn default_consensus_topic() -> String {
    "cardano.consensus.offers".to_string()
}

fn default_block_wanted_topic() -> String {
    "cardano.consensus.wants".to_string()
}

fn default_target_peer_count() -> usize {
    15
}

fn default_min_hot_peers() -> usize {
    3
}

fn default_peer_sharing_enabled() -> bool {
    true
}

fn default_churn_interval_secs() -> u64 {
    600
}

fn default_peer_sharing_timeout_secs() -> u64 {
    10
}

fn default_connect_timeout_secs() -> u64 {
    15
}

fn default_ipv6_enabled() -> bool {
    false
}

fn default_allow_non_public_peer_addrs() -> bool {
    true
}

fn default_discovery_interval_secs() -> u64 {
    60
}

fn default_peer_sharing_cooldown_secs() -> u64 {
    30
}

impl InterfaceConfig {
    pub fn try_load(config: &Config) -> Result<Self> {
        let full_config = Config::builder()
            .add_source(config::File::from_str(
                include_str!("../config.default.toml"),
                config::FileFormat::Toml,
            ))
            .add_source(config.clone())
            .build()?;
        Ok(full_config.try_deserialize()?)
    }
}
