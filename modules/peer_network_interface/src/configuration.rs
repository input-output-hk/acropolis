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

// Block flow mode determines how the peer-network-interface handles block synchronization.
//
// - Direct: blocks are automatically fetched and published as they come in from peers.
// - Consensus: block headers are first offered to a consensus module which decides which to fetch.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BlockFlowMode {
    #[default]
    Direct,
    Consensus,
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
    #[serde(default)]
    pub block_flow_mode: BlockFlowMode,
    #[serde(default = "default_consensus_topic")]
    pub consensus_topic: String,
    #[serde(default = "default_block_wanted_topic")]
    pub block_wanted_topic: String,
}

fn default_consensus_topic() -> String {
    "cardano.consensus.offers".to_string()
}

fn default_block_wanted_topic() -> String {
    "cardano.consensus.wants".to_string()
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
