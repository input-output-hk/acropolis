use acropolis_common::Point;
use anyhow::Result;
use config::Config;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read {0}: {1}")]
    ReadFile(PathBuf, std::io::Error),

    #[error("Failed to parse {0}: {1}")]
    ParseJson(PathBuf, serde_json::Error),

    #[error("Snapshot not found for epoch {0}")]
    SnapshotNotFound(u64),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StartupConfig {
    pub network_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BootstrapConfig {
    pub startup: StartupConfig,
    pub data_dir: PathBuf,
    pub epoch: u64, // The target epoch, straight from TOML
    pub snapshot_topic: String,
    pub bootstrapped_subscribe_topic: String,
    pub sync_command_topic: String,
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

    pub fn network_dir(&self) -> PathBuf {
        self.data_dir.join(&self.startup.network_name)
    }

    pub fn snapshot(&self) -> Result<Snapshot, ConfigError> {
        Snapshot::load_for_epoch(&self.network_dir(), self.epoch)
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
    #[serde(
        deserialize_with = "deserialize_point",
        serialize_with = "serialize_point"
    )]
    pub point: Point,
    pub url: String,
}

impl Snapshot {
    pub fn path(network_dir: &Path) -> PathBuf {
        network_dir.join("snapshots.json")
    }

    pub fn load_all(network_dir: &Path) -> Result<Vec<Self>, ConfigError> {
        let path = Self::path(network_dir);
        let content =
            fs::read_to_string(&path).map_err(|e| ConfigError::ReadFile(path.clone(), e))?;
        serde_json::from_str(&content).map_err(|e| ConfigError::ParseJson(path, e))
    }

    pub fn load_for_epoch(network_dir: &Path, epoch: u64) -> Result<Self, ConfigError> {
        Self::load_all(network_dir)?
            .into_iter()
            .find(|s| s.epoch == epoch)
            .ok_or(ConfigError::SnapshotNotFound(epoch))
    }

    pub fn cbor_path(&self, network_dir: &Path) -> PathBuf {
        let filename = format!(
            "{}.{}.cbor",
            self.point.slot(),
            self.point.hash().expect("snapshot point must have hash")
        );
        network_dir.join(filename)
    }
}

fn deserialize_point<'de, D>(deserializer: D) -> Result<Point, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let (slot_str, hash_str) = s
        .split_once('.')
        .ok_or_else(|| D::Error::custom("invalid point format, expected 'slot.hash'"))?;

    Ok(Point::Specific {
        slot: slot_str.parse().map_err(|e| D::Error::custom(format!("invalid slot: {e}")))?,
        hash: hash_str.parse().map_err(|e| D::Error::custom(format!("invalid hash: {e}")))?,
    })
}

fn serialize_point<S>(point: &Point, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match point {
        Point::Origin => serializer.serialize_str("origin"),
        Point::Specific { slot, hash } => serializer.serialize_str(&format!("{slot}.{hash}")),
    }
}
