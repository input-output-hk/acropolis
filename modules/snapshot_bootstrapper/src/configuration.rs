//! Configuration types for the snapshot bootstrapper.
//!
//! This module provides:
//! - `SnapshotConfig` - Main application configuration (from TOML/environment)
//! - `NetworkConfig` - Network-specific config from `config.json`
//! - `SnapshotFileMetadata` - Snapshot metadata from `snapshots.json`
//! - `NoncesFileData` - Nonce state from `nonces.json`
//! - `HeadersFileData` / `HeaderFileData` - Header references and CBOR data
//! - `BootstrapFiles` - Combined loader for all bootstrap data
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

    #[error("Cannot read nonces file {0}: {1}")]
    ReadNoncesFile(PathBuf, io::Error),

    #[error("Cannot read headers file {0}: {1}")]
    ReadHeadersFile(PathBuf, io::Error),

    #[error("Cannot read header CBOR file {0}: {1}")]
    ReadHeaderCborFile(PathBuf, io::Error),

    #[error("Failed to parse network config {0}: {1}")]
    MalformedNetworkConfig(PathBuf, serde_json::Error),

    #[error("Failed to parse snapshots JSON file {0}: {1}")]
    MalformedSnapshotsFile(PathBuf, serde_json::Error),

    #[error("Failed to parse nonces JSON file {0}: {1}")]
    MalformedNoncesFile(PathBuf, serde_json::Error),

    #[error("Failed to parse headers JSON file {0}: {1}")]
    MalformedHeadersFile(PathBuf, serde_json::Error),

    #[error("Invalid hex string: {0}")]
    InvalidHex(String),

    #[error("Invalid point format (expected 'slot.hash'): {0}")]
    InvalidPointFormat(String),

    #[error("Header not found for point: {0}")]
    HeaderNotFound(String),
}

/// Main configuration for the snapshot bootstrapper module.
///
/// Loaded from TOML config file with defaults from `config.default.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotConfig {
    /// Network name (e.g., "mainnet", "preprod")
    pub network: String,

    /// Base directory for data files
    pub data_dir: String,

    /// Topic to publish snapshot data on
    pub snapshot_topic: String,

    /// Topic to subscribe for bootstrap completion signal
    pub bootstrapped_subscribe_topic: String,

    /// Topic to publish completion message on
    pub completion_topic: String,

    /// Download configuration
    #[serde(default)]
    pub download: DownloadConfig,
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

    /// Returns the network-specific data directory path.
    pub fn network_dir(&self) -> String {
        format!("{}/{}", self.data_dir, self.network)
    }

    /// Returns the path to `config.json`.
    pub fn config_path(&self) -> String {
        format!("{}/config.json", self.network_dir())
    }

    /// Returns the path to `snapshots.json`.
    pub fn snapshots_path(&self) -> String {
        format!("{}/snapshots.json", self.network_dir())
    }

    /// Returns the path to `nonces.json`.
    pub fn nonces_path(&self) -> String {
        format!("{}/nonces.json", self.network_dir())
    }

    /// Returns the path to `headers.json`.
    pub fn headers_path(&self) -> String {
        format!("{}/headers.json", self.network_dir())
    }
}

/// Configuration for snapshot downloads.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DownloadConfig {
    /// Total request timeout in seconds (default: 300)
    #[serde(default = "DownloadConfig::default_timeout_secs")]
    pub timeout_secs: u64,

    /// Connection timeout in seconds (default: 30)
    #[serde(default = "DownloadConfig::default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,

    /// How often to log download progress in chunks (default: 200)
    #[serde(default = "DownloadConfig::default_progress_log_interval")]
    pub progress_log_interval: u64,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            timeout_secs: Self::default_timeout_secs(),
            connect_timeout_secs: Self::default_connect_timeout_secs(),
            progress_log_interval: Self::default_progress_log_interval(),
        }
    }
}

impl DownloadConfig {
    fn default_timeout_secs() -> u64 {
        300
    }

    fn default_connect_timeout_secs() -> u64 {
        30
    }

    fn default_progress_log_interval() -> u64 {
        200
    }
}

/// Network configuration from `config.json`.
///
/// Example:
/// ```json
/// {
///   "snapshot": 509,
///   "points": [
///     { "epoch": 507, "id": "670ca68c...", "slot": 134092758 }
///   ]
/// }
/// ```
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfig {
    /// The target epoch to bootstrap to
    pub snapshot: u64,

    /// Known epoch boundary points (reserved for future use)
    #[serde(default)]
    pub points: Vec<Point>,
}

impl NetworkConfig {
    pub fn read_from_file(path: &str) -> Result<Self, ConfigError> {
        let path_buf = PathBuf::from(path);
        let content = fs::read_to_string(&path_buf)
            .map_err(|e| ConfigError::ReadNetworkConfig(path_buf.clone(), e))?;

        serde_json::from_str(&content).map_err(|e| ConfigError::MalformedNetworkConfig(path_buf, e))
    }
}

/// A point representing an epoch boundary (reserved for future use).
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub epoch: u64,
    pub id: String,
    pub slot: u64,
}

/// Snapshot file metadata from `snapshots.json`.
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

        serde_json::from_str(&content).map_err(|e| ConfigError::MalformedSnapshotsFile(path_buf, e))
    }

    /// Returns the local file path for this snapshot.
    pub fn file_path(&self, network_dir: &str) -> String {
        format!("{}/{}.cbor", network_dir, self.point)
    }

    /// Find a snapshot by epoch.
    pub fn find_by_epoch(snapshots: &[Self], epoch: u64) -> Option<Self> {
        snapshots.iter().find(|s| s.epoch == epoch).cloned()
    }
}

/// Nonces data from `nonces.json`.
///
/// Matches Amaru's `InitialNonces` format:
/// ```json
/// {
///   "at": "134956789.6558deef...",
///   "active": "0b9e320e...",
///   "candidate": "6cc4dafe...",
///   "evolving": "f5589f01...",
///   "tail": "29011cc1..."
/// }
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NoncesFileData {
    /// The point at which these nonces are valid ("slot.hash" format)
    #[serde(deserialize_with = "deserialize_point_string")]
    pub at: String,

    /// Active nonce (hex-encoded 32-byte hash)
    pub active: String,

    /// Candidate nonce (hex-encoded 32-byte hash)
    pub candidate: String,

    /// Evolving nonce (hex-encoded 32-byte hash)
    pub evolving: String,

    /// Tail - previous epoch's last block header hash (hex-encoded 32-byte hash)
    pub tail: String,
}

/// Deserializer that validates "slot.hash" format.
fn deserialize_point_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if parse_point_string(&s).is_none() {
        return Err(serde::de::Error::custom(format!(
            "invalid point format: {}",
            s
        )));
    }
    Ok(s)
}

impl NoncesFileData {
    pub fn read_from_file(path: &str) -> Result<Self, ConfigError> {
        let path_buf = PathBuf::from(path);
        let content = fs::read_to_string(&path_buf)
            .map_err(|e| ConfigError::ReadNoncesFile(path_buf.clone(), e))?;

        serde_json::from_str(&content).map_err(|e| ConfigError::MalformedNoncesFile(path_buf, e))
    }

    /// Parse the "at" field into (slot, hash).
    pub fn parse_at(&self) -> Option<(u64, &str)> {
        parse_point_string(&self.at)
    }

    /// Decode a hex string into 32 bytes.
    fn decode_hash32(hex_str: &str) -> Result<[u8; 32], ConfigError> {
        let bytes =
            hex::decode(hex_str).map_err(|_| ConfigError::InvalidHex(hex_str.to_string()))?;

        bytes.try_into().map_err(|_| {
            ConfigError::InvalidHex(format!("expected 32 bytes, got {}", hex_str.len() / 2))
        })
    }

    fn active_bytes(&self) -> Result<[u8; 32], ConfigError> {
        Self::decode_hash32(&self.active)
    }

    fn candidate_bytes(&self) -> Result<[u8; 32], ConfigError> {
        Self::decode_hash32(&self.candidate)
    }

    fn evolving_bytes(&self) -> Result<[u8; 32], ConfigError> {
        Self::decode_hash32(&self.evolving)
    }

    fn tail_bytes(&self) -> Result<[u8; 32], ConfigError> {
        Self::decode_hash32(&self.tail)
    }
}

/// Raw nonces as bytes, ready for conversion to domain types.
///
/// This is an intermediate representation before converting to
/// the actual `Nonces` type from amaru_kernel.
#[derive(Debug, Clone)]
pub struct NoncesData {
    pub epoch: u64,
    pub active: [u8; 32],
    pub evolving: [u8; 32],
    pub candidate: [u8; 32],
    pub tail: [u8; 32],
}

impl NoncesData {
    /// Build from file data and epoch.
    fn from_file_data(data: &NoncesFileData, epoch: u64) -> Result<Self, ConfigError> {
        Ok(Self {
            epoch,
            active: data.active_bytes()?,
            evolving: data.evolving_bytes()?,
            candidate: data.candidate_bytes()?,
            tail: data.tail_bytes()?,
        })
    }

    /// Convert to hex strings for logging.
    pub fn to_hex_strings(&self) -> (String, String, String, String) {
        (
            hex::encode(self.active),
            hex::encode(self.evolving),
            hex::encode(self.candidate),
            hex::encode(self.tail),
        )
    }
}

/// List of header points from `headers.json`.
#[derive(Debug, Clone)]
pub struct HeadersFileData {
    points: Vec<String>,
}

impl HeadersFileData {
    pub fn read_from_file(path: &str) -> Result<Self, ConfigError> {
        let path_buf = PathBuf::from(path);
        let content = fs::read_to_string(&path_buf)
            .map_err(|e| ConfigError::ReadHeadersFile(path_buf.clone(), e))?;

        let points: Vec<String> = serde_json::from_str(&content)
            .map_err(|e| ConfigError::MalformedHeadersFile(path_buf, e))?;

        Ok(Self { points })
    }

    /// Find a point by its hash.
    fn find_by_hash(&self, hash: &str) -> Option<&String> {
        self.points.iter().find(|p| parse_point_string(p).map(|(_, h)| h == hash).unwrap_or(false))
    }
}

/// Data from a `header.{slot}.{hash}.cbor` file.
#[derive(Debug, Clone)]
pub struct HeaderFileData {
    pub cbor_bytes: Vec<u8>,
    pub slot: u64,
    pub hash: String,
}

impl HeaderFileData {
    /// Read from network dir using a point string.
    fn read_from_point(network_dir: &str, point: &str) -> Result<Self, ConfigError> {
        let path = format!("{}/headers/header.{}.cbor", network_dir, point);
        let path_buf = PathBuf::from(&path);
        let binding = path_buf.clone();

        let filename = binding
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ConfigError::InvalidPointFormat(path.clone()))?;

        let (slot, hash) = Self::parse_filename(filename)
            .ok_or_else(|| ConfigError::InvalidPointFormat(filename.to_string()))?;

        let cbor_bytes =
            fs::read(&path_buf).map_err(|e| ConfigError::ReadHeaderCborFile(path_buf, e))?;

        Ok(Self {
            cbor_bytes,
            slot,
            hash: hash.to_string(),
        })
    }

    /// Parse filename: "header.{slot}.{hash}.cbor" -> (slot, hash)
    fn parse_filename(filename: &str) -> Option<(u64, &str)> {
        let inner = filename.strip_prefix("header.")?.strip_suffix(".cbor")?;
        parse_point_string(inner)
    }

    /// Get the hash as 32 bytes.
    pub fn hash_bytes(&self) -> Result<[u8; 32], ConfigError> {
        NoncesFileData::decode_hash32(&self.hash)
    }
}

/// Combined loader for all bootstrap data files.
#[derive(Debug)]
pub struct BootstrapFiles {
    pub network_config: NetworkConfig,
    pub snapshots: Vec<SnapshotFileMetadata>,
    pub nonces: NoncesFileData,
    pub target_header: HeaderFileData,
}

impl BootstrapFiles {
    /// Load all bootstrap files for the given configuration.
    pub fn load(cfg: &SnapshotConfig) -> Result<Self, ConfigError> {
        let network_config = NetworkConfig::read_from_file(&cfg.config_path())?;
        let snapshots = SnapshotFileMetadata::read_all_from_file(&cfg.snapshots_path())?;
        let nonces = NoncesFileData::read_from_file(&cfg.nonces_path())?;
        let headers = HeadersFileData::read_from_file(&cfg.headers_path())?;

        // Find target header from nonces "at" field
        let (_, nonces_hash) =
            nonces.parse_at().ok_or_else(|| ConfigError::InvalidPointFormat(nonces.at.clone()))?;

        let target_point = headers
            .find_by_hash(nonces_hash)
            .ok_or_else(|| ConfigError::HeaderNotFound(nonces_hash.to_string()))?;

        let target_header = HeaderFileData::read_from_point(&cfg.network_dir(), target_point)?;

        Ok(Self {
            network_config,
            snapshots,
            nonces,
            target_header,
        })
    }

    /// Get the target epoch for bootstrapping.
    pub fn target_epoch(&self) -> u64 {
        self.network_config.snapshot
    }

    /// Get the snapshot metadata for the target epoch.
    pub fn target_snapshot(&self) -> Option<SnapshotFileMetadata> {
        SnapshotFileMetadata::find_by_epoch(&self.snapshots, self.target_epoch())
    }

    /// Build NoncesData from the loaded files.
    pub fn build_nonces(&self) -> Result<NoncesData, ConfigError> {
        NoncesData::from_file_data(&self.nonces, self.target_epoch())
    }
}

/// Parse a "slot.hash" point string into (slot, hash).
fn parse_point_string(s: &str) -> Option<(u64, &str)> {
    let (slot_str, hash) = s.split_once('.')?;
    let slot = slot_str.parse().ok()?;
    Some((slot, hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_network_config(dir: &std::path::Path, snapshot: u64) -> PathBuf {
        let config = serde_json::json!({
            "snapshot": snapshot,
            "points": []
        });

        let path = dir.join("config.json");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(serde_json::to_string_pretty(&config).unwrap().as_bytes()).unwrap();
        path
    }

    fn create_test_nonces(dir: &std::path::Path) -> PathBuf {
        let nonces = serde_json::json!({
            "at": "134956789.6558deef007ba372a414466e49214368c17c1f8428093193fc187d1c4587053c",
            "active": "0b9e320e63bf995b81287ce7a624b6735d98b083cc1a0e2ae8b08b680c79c983",
            "candidate": "6cc4dafecbe0d593ca0dee64518542f5faa741538791ac7fc2d5008f32d5c4d5",
            "evolving": "f5589f01dd0efd0add0c58e8b27dc73ba3fcd662d9026b3fedbf06c648adb313",
            "tail": "29011cc1320d03b3da0121236dc66e6bc391feef4bb1d506a7fb20e769d6a494"
        });

        let path = dir.join("nonces.json");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(serde_json::to_string_pretty(&nonces).unwrap().as_bytes()).unwrap();
        path
    }

    fn create_test_snapshots(dir: &std::path::Path, epochs: Vec<u64>) -> PathBuf {
        let snapshots: Vec<_> = epochs
            .iter()
            .map(|epoch| {
                serde_json::json!({
                    "epoch": epoch,
                    "point": "134956789.6558deef007ba372a414466e49214368c17c1f8428093193fc187d1c4587053c",
                    "url": format!("https://example.com/snapshot_{}.cbor.gz", epoch)
                })
            })
            .collect();

        let path = dir.join("snapshots.json");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(serde_json::to_string_pretty(&snapshots).unwrap().as_bytes()).unwrap();
        path
    }

    // --- SnapshotConfig Tests ---
    #[test]
    fn test_snapshot_config_paths() {
        let config = SnapshotConfig {
            network: "mainnet".to_string(),
            data_dir: "./data".to_string(),
            snapshot_topic: "snapshot".to_string(),
            bootstrapped_subscribe_topic: "bootstrapped".to_string(),
            completion_topic: "completion".to_string(),
            download: DownloadConfig::default(),
        };

        assert_eq!(config.network_dir(), "./data/mainnet");
        assert_eq!(config.config_path(), "./data/mainnet/config.json");
        assert_eq!(config.snapshots_path(), "./data/mainnet/snapshots.json");
        assert_eq!(config.nonces_path(), "./data/mainnet/nonces.json");
        assert_eq!(config.headers_path(), "./data/mainnet/headers.json");
    }

    // --- NetworkConfig Tests ---
    #[test]
    fn test_network_config_read() {
        let temp_dir = TempDir::new().unwrap();
        create_test_network_config(temp_dir.path(), 509);

        let config =
            NetworkConfig::read_from_file(temp_dir.path().join("config.json").to_str().unwrap())
                .unwrap();

        assert_eq!(config.snapshot, 509);
    }

    #[test]
    fn test_network_config_missing_file() {
        let result = NetworkConfig::read_from_file("/nonexistent/config.json");
        assert!(matches!(result, Err(ConfigError::ReadNetworkConfig(_, _))));
    }

    // --- SnapshotFileMetadata Tests ---
    #[test]
    fn test_snapshot_metadata_find_by_epoch() {
        let temp_dir = TempDir::new().unwrap();
        create_test_snapshots(temp_dir.path(), vec![507, 508, 509]);

        let snapshots = SnapshotFileMetadata::read_all_from_file(
            temp_dir.path().join("snapshots.json").to_str().unwrap(),
        )
        .unwrap();

        let found = SnapshotFileMetadata::find_by_epoch(&snapshots, 508).unwrap();
        assert_eq!(found.epoch, 508);

        assert!(SnapshotFileMetadata::find_by_epoch(&snapshots, 999).is_none());
    }

    #[test]
    fn test_snapshot_metadata_file_path() {
        let meta = SnapshotFileMetadata {
            epoch: 509,
            point: "123.abc".to_string(),
            url: "https://example.com".to_string(),
        };

        assert_eq!(
            meta.file_path("/data/mainnet"),
            "/data/mainnet/123.abc.cbor"
        );
    }

    // --- NoncesFileData Tests ---
    #[test]
    fn test_nonces_file_data() {
        let temp_dir = TempDir::new().unwrap();
        create_test_nonces(temp_dir.path());

        let nonces =
            NoncesFileData::read_from_file(temp_dir.path().join("nonces.json").to_str().unwrap())
                .unwrap();

        let (slot, hash) = nonces.parse_at().unwrap();
        assert_eq!(slot, 134956789);
        assert!(hash.starts_with("6558deef"));
    }

    #[test]
    fn test_nonces_data_to_hex() {
        let temp_dir = TempDir::new().unwrap();
        create_test_nonces(temp_dir.path());

        let file_data =
            NoncesFileData::read_from_file(temp_dir.path().join("nonces.json").to_str().unwrap())
                .unwrap();

        let nonces = NoncesData::from_file_data(&file_data, 509).unwrap();
        let (active, evolving, candidate, tail) = nonces.to_hex_strings();

        assert_eq!(active.len(), 64);
        assert_eq!(evolving.len(), 64);
        assert_eq!(candidate.len(), 64);
        assert_eq!(tail.len(), 64);
    }

    #[test]
    fn test_decode_hash32_invalid() {
        assert!(matches!(
            NoncesFileData::decode_hash32("not_hex!"),
            Err(ConfigError::InvalidHex(_))
        ));

        assert!(matches!(
            NoncesFileData::decode_hash32("abcd"),
            Err(ConfigError::InvalidHex(_))
        ));
    }

    // --- Utility Tests ---
    #[test]
    fn test_parse_point_string() {
        let (slot, hash) = parse_point_string("134956789.abc123def").unwrap();
        assert_eq!(slot, 134956789);
        assert_eq!(hash, "abc123def");

        assert!(parse_point_string("invalid").is_none());
        assert!(parse_point_string("not_a_number.abc").is_none());
    }
}
