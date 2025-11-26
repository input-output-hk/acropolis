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
    Snapshot,
    Dynamic,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct InterfaceConfig {
    pub block_topic: String,
    pub sync_point: SyncPoint,
    pub snapshot_completion_topic: String,
    pub genesis_completion_topic: String,
    pub sync_command_topic: String,
    pub node_addresses: Vec<String>,
    pub magic_number: u64,
    pub cache_dir: PathBuf,
    #[serde(flatten)]
    pub genesis_values: Option<GenesisValues>,
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
