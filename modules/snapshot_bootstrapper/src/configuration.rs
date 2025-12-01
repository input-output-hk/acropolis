use anyhow::Result;
use config::Config;
use serde::{Deserialize, Serialize};

/// Bootstrap module configuration (from TOML).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BootstrapConfig {
    pub network: String,
    pub data_dir: String,
    pub snapshot_topic: String,
    pub bootstrapped_subscribe_topic: String,
    pub completion_topic: String,
    #[serde(default)]
    pub download: DownloadConfig,
}

impl BootstrapConfig {
    pub fn try_load(config: &Config) -> Result<Self> {
        let full = Config::builder()
            .add_source(config::File::from_str(
                include_str!("../config.default.toml"),
                config::FileFormat::Toml,
            ))
            .add_source(config.clone())
            .build()?;
        Ok(full.try_deserialize()?)
    }

    pub fn network_dir(&self) -> String {
        format!("{}/{}", self.data_dir, self.network)
    }
}

/// Download settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DownloadConfig {
    #[serde(default = "defaults::timeout")]
    pub timeout_secs: u64,
    #[serde(default = "defaults::connect_timeout")]
    pub connect_timeout_secs: u64,
    #[serde(default = "defaults::progress_interval")]
    pub progress_log_interval: u64,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            timeout_secs: defaults::timeout(),
            connect_timeout_secs: defaults::connect_timeout(),
            progress_log_interval: defaults::progress_interval(),
        }
    }
}

mod defaults {
    pub fn timeout() -> u64 {
        300
    }
    pub fn connect_timeout() -> u64 {
        30
    }
    pub fn progress_interval() -> u64 {
        200
    }
}

/// Snapshot entry from snapshots.json.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Snapshot {
    pub epoch: u64,
    pub point: String,
    pub url: String,
}

impl Snapshot {
    pub fn file_path(&self, network_dir: &str) -> String {
        format!("{}/{}.cbor", network_dir, self.point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_config_paths() {
        let config = BootstrapConfig {
            network: "mainnet".to_string(),
            data_dir: "./data".to_string(),
            snapshot_topic: "snapshot".to_string(),
            bootstrapped_subscribe_topic: "bootstrapped".to_string(),
            completion_topic: "completion".to_string(),
            download: DownloadConfig::default(),
        };

        assert_eq!(config.network_dir(), "./data/mainnet");
    }

    #[test]
    fn test_download_config_defaults() {
        let config = DownloadConfig::default();
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.connect_timeout_secs, 30);
        assert_eq!(config.progress_log_interval, 200);
    }
}
