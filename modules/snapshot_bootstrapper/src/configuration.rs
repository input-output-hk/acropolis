use anyhow::Result;
use config::Config;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Cannot read network config file {0}: {1}")]
    ReadNetworkConfig(PathBuf, io::Error),

    #[error("Cannot read snapshots metadata file {0}: {1}")]
    ReadSnapshotsFile(PathBuf, io::Error),

    #[error("Failed to parse network config {0}: {1}")]
    MalformedNetworkConfig(PathBuf, serde_json::Error),

    #[error("Failed to parse snapshots JSON file {0}: {1}")]
    MalformedSnapshotsFile(PathBuf, serde_json::Error),
}

/// Configuration for the snapshot bootstrapper
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotConfig {
    pub network: String,
    pub data_dir: String,
    pub startup_topic: String,
    pub snapshot_topic: String,
    pub bootstrapped_subscribe_topic: String,
    pub completion_topic: String,
}

impl SnapshotConfig {
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

    pub fn network_dir(&self) -> String {
        format!("{}/{}", self.data_dir, self.network)
    }

    pub fn config_path(&self) -> String {
        format!("{}/config.json", self.network_dir())
    }

    pub fn snapshots_path(&self) -> String {
        format!("{}/snapshots.json", self.network_dir())
    }
}

/// Network configuration file (config.json)
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfig {
    pub snapshots: Vec<u64>,
    pub points: Vec<Point>,
}

impl NetworkConfig {
    pub fn read_from_file(path: &str) -> Result<Self, ConfigError> {
        let path_buf = PathBuf::from(path);
        let content = fs::read_to_string(&path_buf)
            .map_err(|e| ConfigError::ReadNetworkConfig(path_buf.clone(), e))?;

        let config: NetworkConfig = serde_json::from_str(&content)
            .map_err(|e| ConfigError::MalformedNetworkConfig(path_buf, e))?;

        Ok(config)
    }
}

/// Point
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub epoch: u64,
    pub id: String,
    pub slot: u64,
}

/// Snapshot metadata from snapshots.json
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SnapshotFileMetadata {
    pub epoch: u64,
    pub point: String,
    pub url: String,
}

impl SnapshotFileMetadata {
    pub fn read_all_from_file(path: &str) -> Result<Vec<Self>, ConfigError> {
        let path_buf = PathBuf::from(path);
        let content = fs::read_to_string(&path_buf)
            .map_err(|e| ConfigError::ReadSnapshotsFile(path_buf.clone(), e))?;

        let snapshots: Vec<SnapshotFileMetadata> = serde_json::from_str(&content)
            .map_err(|e| ConfigError::MalformedSnapshotsFile(path_buf, e))?;

        Ok(snapshots)
    }

    pub fn parse_point(&self) -> Option<(u64, String)> {
        let parts: Vec<&str> = self.point.splitn(2, '.').collect();
        if parts.len() == 2 {
            let slot = parts[0].parse().ok()?;
            let hash = parts[1].to_string();
            Some((slot, hash))
        } else {
            None
        }
    }

    pub fn file_path(&self, network_dir: &str) -> String {
        format!("{}/{}.cbor", network_dir, self.point)
    }

    pub fn filter_by_epochs(snapshots: &[Self], epochs: &[u64]) -> Vec<Self> {
        snapshots.iter().filter(|s| epochs.contains(&s.epoch)).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_test_network_config(dir: &Path, snapshots: Vec<u64>) -> PathBuf {
        let config = NetworkConfig {
            snapshots,
            points: vec![Point {
                epoch: 500,
                id: "test_block_hash".to_string(),
                slot: 12345678,
            }],
        };

        let config_path = dir.join("config.json");
        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(serde_json::to_string_pretty(&config).unwrap().as_bytes()).unwrap();
        config_path
    }

    fn create_test_snapshots_metadata(dir: &Path, epochs: Vec<u64>, base_url: &str) -> PathBuf {
        let snapshots: Vec<SnapshotFileMetadata> = epochs
            .iter()
            .map(|epoch| SnapshotFileMetadata {
                epoch: *epoch,
                point: format!("point_{}", epoch),
                url: format!("{}/snapshot_{}.cbor.gz", base_url, epoch),
            })
            .collect();

        let snapshots_path = dir.join("snapshots.json");
        let mut file = fs::File::create(&snapshots_path).unwrap();
        file.write_all(serde_json::to_string_pretty(&snapshots).unwrap().as_bytes()).unwrap();
        snapshots_path
    }

    #[test]
    fn test_snapshot_config_network_dir() {
        let config = SnapshotConfig {
            network: "mainnet".to_string(),
            data_dir: "./data".to_string(),
            startup_topic: "startup".to_string(),
            snapshot_topic: "snapshot".to_string(),
            bootstrapped_subscribe_topic: "bootstrapped".to_string(),
            completion_topic: "completion".to_string(),
        };

        assert_eq!(config.network_dir(), "./data/mainnet");
    }

    #[test]
    fn test_snapshot_config_config_path() {
        let config = SnapshotConfig {
            network: "preprod".to_string(),
            data_dir: "/var/data".to_string(),
            startup_topic: "startup".to_string(),
            snapshot_topic: "snapshot".to_string(),
            bootstrapped_subscribe_topic: "bootstrapped".to_string(),
            completion_topic: "completion".to_string(),
        };

        assert_eq!(config.config_path(), "/var/data/preprod/config.json");
    }

    #[test]
    fn test_snapshot_config_snapshots_path() {
        let config = SnapshotConfig {
            network: "mainnet".to_string(),
            data_dir: "./data".to_string(),
            startup_topic: "startup".to_string(),
            snapshot_topic: "snapshot".to_string(),
            bootstrapped_subscribe_topic: "bootstrapped".to_string(),
            completion_topic: "completion".to_string(),
        };

        assert_eq!(config.snapshots_path(), "./data/mainnet/snapshots.json");
    }

    #[test]
    fn test_snapshot_file_metadata_file_path() {
        let metadata = SnapshotFileMetadata {
            epoch: 500,
            point: "point_500".to_string(),
            url: "https://example.com/snapshot.cbor.gz".to_string(),
        };

        assert_eq!(
            metadata.file_path("/data/mainnet"),
            "/data/mainnet/point_500.cbor"
        );
    }

    #[test]
    fn test_filter_by_epochs() {
        let all_snapshots = vec![
            SnapshotFileMetadata {
                epoch: 500,
                point: "point_500".to_string(),
                url: "url1".to_string(),
            },
            SnapshotFileMetadata {
                epoch: 501,
                point: "point_501".to_string(),
                url: "url2".to_string(),
            },
            SnapshotFileMetadata {
                epoch: 502,
                point: "point_502".to_string(),
                url: "url3".to_string(),
            },
        ];

        let filtered = SnapshotFileMetadata::filter_by_epochs(&all_snapshots, &[500, 502]);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].epoch, 500);
        assert_eq!(filtered[1].epoch, 502);
    }

    #[test]
    fn test_read_network_config_success() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = create_test_network_config(temp_dir.path(), vec![500, 501]);

        let result = NetworkConfig::read_from_file(config_path.to_str().unwrap());
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.snapshots, vec![500, 501]);
        assert_eq!(config.points.len(), 1);
    }

    #[test]
    fn test_read_network_config_missing_file() {
        let result = NetworkConfig::read_from_file("/nonexistent/config.json");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConfigError::ReadNetworkConfig(_, _)
        ));
    }

    #[test]
    fn test_read_network_config_malformed_json() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(b"{ invalid json }").unwrap();

        let result = NetworkConfig::read_from_file(config_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConfigError::MalformedNetworkConfig(_, _)
        ));
    }

    #[test]
    fn test_read_snapshots_metadata_success() {
        let temp_dir = TempDir::new().unwrap();
        let snapshots_path =
            create_test_snapshots_metadata(temp_dir.path(), vec![500, 501], "https://example.com");

        let result = SnapshotFileMetadata::read_all_from_file(snapshots_path.to_str().unwrap());
        assert!(result.is_ok());

        let snapshots = result.unwrap();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].epoch, 500);
        assert_eq!(snapshots[1].epoch, 501);
    }

    #[test]
    fn test_read_snapshots_metadata_missing_file() {
        let result = SnapshotFileMetadata::read_all_from_file("/nonexistent/snapshots.json");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConfigError::ReadSnapshotsFile(_, _)
        ));
    }

    #[test]
    fn test_corrupted_config_json_fails_gracefully() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(b"{\"snapshots\": [500, 501]").unwrap();

        let result = NetworkConfig::read_from_file(config_path.to_str().unwrap());
        assert!(result.is_err());

        if let Err(ConfigError::MalformedNetworkConfig(path, _)) = result {
            assert_eq!(path, config_path);
        } else {
            panic!("Expected MalformedNetworkConfig error");
        }
    }

    #[test]
    fn test_corrupted_snapshots_json_fails_gracefully() {
        let temp_dir = TempDir::new().unwrap();
        let snapshots_path = temp_dir.path().join("snapshots.json");
        let mut file = fs::File::create(&snapshots_path).unwrap();
        file.write_all(b"[{\"epoch\": 500}").unwrap();

        let result = SnapshotFileMetadata::read_all_from_file(snapshots_path.to_str().unwrap());
        assert!(result.is_err());

        if let Err(ConfigError::MalformedSnapshotsFile(path, _)) = result {
            assert_eq!(path, snapshots_path);
        } else {
            panic!("Expected MalformedSnapshotsFile error");
        }
    }
}
