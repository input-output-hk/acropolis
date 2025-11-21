use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use acropolis_common::{
    messages::{CardanoMessage, Message},
    snapshot::{
        streaming_snapshot::{
            DRepCallback, DRepInfo, GovernanceProposal, PoolCallback, PoolInfo, ProposalCallback,
            SnapshotCallbacks, SnapshotMetadata, StakeCallback, UtxoCallback, UtxoEntry,
        },
        StreamingSnapshotParser,
    },
    stake_addresses::AccountState,
    BlockHash, BlockInfo, BlockStatus, Era,
};
use anyhow::{bail, Result};
use async_compression::tokio::bufread::GzipDecoder;
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::BufReader;
use tokio::time::Instant;
use tracing::{error, info, info_span, Instrument};

const DEFAULT_SNAPSHOT_TOPIC: &str = "cardano.snapshot";
const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.start";
const DEFAULT_COMPLETION_TOPIC: &str = "cardano.snapshot.complete";
const DEFAULT_BOOTSTRAPPED_TOPIC: &str = "cardano.sequence.bootstrapped";

#[derive(Debug, Error)]
pub enum SnapshotBootstrapError {
    #[error("Cannot read network config file {0}: {1}")]
    ReadNetworkConfig(PathBuf, io::Error),

    #[error("Cannot read snapshots metadata file {0}: {1}")]
    ReadSnapshotsFile(PathBuf, io::Error),

    #[error("Failed to parse network config {0}: {1}")]
    MalformedNetworkConfig(PathBuf, serde_json::Error),

    #[error("Failed to parse snapshots JSON file {0}: {1}")]
    MalformedSnapshotsFile(PathBuf, serde_json::Error),

    #[error("Cannot create directory {0}: {1}")]
    CreateDirectory(PathBuf, io::Error),

    #[error("Failed to download snapshot from {0}: {1}")]
    DownloadError(String, reqwest::Error),

    #[error("Download failed from {0}: HTTP status {1}")]
    DownloadInvalidStatusCode(String, reqwest::StatusCode),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Snapshot parsing failed: {0}")]
    ParseError(String),
}

/// Configuration for the snapshot bootstrapper
#[derive(Debug, Clone)]
struct SnapshotConfig {
    network: String,
    data_dir: String,
    startup_topic: String,
    snapshot_topic: String,
    bootstrapped_topic: String,
    completion_topic: String,
}

impl SnapshotConfig {
    fn try_load(config: &Config) -> Result<Self> {
        Ok(Self {
            network: config.get_string("network").unwrap_or_else(|_| "mainnet".to_string()),
            data_dir: config.get_string("data-dir").unwrap_or_else(|_| "./data".to_string()),
            startup_topic: config
                .get_string("startup-topic")
                .unwrap_or(DEFAULT_STARTUP_TOPIC.to_string()),
            snapshot_topic: config
                .get_string("snapshot-topic")
                .unwrap_or(DEFAULT_SNAPSHOT_TOPIC.to_string()),
            bootstrapped_topic: config
                .get_string("bootstrapped-subscribe-topic")
                .unwrap_or(DEFAULT_BOOTSTRAPPED_TOPIC.to_string()),
            completion_topic: config
                .get_string("completion-topic")
                .unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string()),
        })
    }

    fn network_dir(&self) -> String {
        format!("{}/{}", self.data_dir, self.network)
    }

    fn config_path(&self) -> String {
        format!("{}/config.json", self.network_dir())
    }

    fn snapshots_path(&self) -> String {
        format!("{}/snapshots.json", self.network_dir())
    }
}

/// Network configuration file (config.json)
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkConfig {
    snapshots: Vec<u64>,
    points: Vec<Point>,
}

/// Point
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Point {
    epoch: u64,
    id: String,
    slot: u64,
}

/// Snapshot metadata from snapshots.json
#[derive(Debug, Deserialize, Serialize, Clone)]
struct SnapshotFileMetadata {
    epoch: u64,
    point: String,
    url: String,
}

/// Handles publishing snapshot data to the message bus
struct SnapshotPublisher {
    context: Arc<Context<Message>>,
    completion_topic: String,
    snapshot_topic: String,
    metadata: Option<SnapshotMetadata>,
    utxo_count: u64,
    pools: Vec<PoolInfo>,
    accounts: Vec<AccountState>,
    dreps: Vec<DRepInfo>,
    proposals: Vec<GovernanceProposal>,
}

impl SnapshotPublisher {
    fn new(
        context: Arc<Context<Message>>,
        completion_topic: String,
        snapshot_topic: String,
    ) -> Self {
        Self {
            context,
            completion_topic,
            snapshot_topic,
            metadata: None,
            utxo_count: 0,
            pools: Vec::new(),
            accounts: Vec::new(),
            dreps: Vec::new(),
            proposals: Vec::new(),
        }
    }

    async fn publish_start(&self) -> Result<()> {
        let message = Arc::new(Message::Snapshot(
            acropolis_common::messages::SnapshotMessage::Startup,
        ));
        self.context.publish(&self.snapshot_topic, message).await
    }

    async fn publish_completion(&self, block_info: BlockInfo) -> Result<()> {
        let message = Arc::new(Message::Cardano((
            block_info,
            CardanoMessage::SnapshotComplete,
        )));
        self.context.publish(&self.completion_topic, message).await
    }
}

impl UtxoCallback for SnapshotPublisher {
    fn on_utxo(&mut self, _utxo: UtxoEntry) -> Result<()> {
        self.utxo_count += 1;

        // Log progress every million UTXOs
        if self.utxo_count.is_multiple_of(1_000_000) {
            info!("Processed {} UTXOs", self.utxo_count);
        }
        // TODO: Accumulate UTXO data if needed or send in chunks to UTXOState processor
        Ok(())
    }
}

impl PoolCallback for SnapshotPublisher {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        info!("Received {} pools", pools.len());
        self.pools.extend(pools);
        Ok(())
    }
}

impl StakeCallback for SnapshotPublisher {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        info!("Received {} accounts", accounts.len());
        self.accounts.extend(accounts);
        Ok(())
    }
}

impl DRepCallback for SnapshotPublisher {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()> {
        info!("Received {} DReps", dreps.len());
        self.dreps.extend(dreps);
        Ok(())
    }
}

impl ProposalCallback for SnapshotPublisher {
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()> {
        info!("Received {} proposals", proposals.len());
        self.proposals.extend(proposals);
        Ok(())
    }
}

impl SnapshotCallbacks for SnapshotPublisher {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        info!("Snapshot metadata for epoch {}", metadata.epoch);
        info!("  UTXOs: {:?}", metadata.utxo_count);
        info!(
            "  Pot balances: treasury={}, reserves={}, deposits={}",
            metadata.pot_balances.treasury,
            metadata.pot_balances.reserves,
            metadata.pot_balances.deposits
        );
        info!(
            "  - Previous epoch blocks: {}",
            metadata.blocks_previous_epoch.len()
        );
        info!(
            "  - Current epoch blocks: {}",
            metadata.blocks_current_epoch.len()
        );

        self.metadata = Some(metadata);
        Ok(())
    }

    fn on_complete(&mut self) -> Result<()> {
        info!("Snapshot parsing completed");
        info!("Final statistics:");
        info!("  - UTXOs processed: {}", self.utxo_count);
        info!("  - Pools: {}", self.pools.len());
        info!("  - Accounts: {}", self.accounts.len());
        info!("  - DReps: {}", self.dreps.len());
        info!("  - Proposals: {}", self.proposals.len());

        // We could send a Resolver reference from here for large data, i.e. the UTXO set,
        // which could be a file reference. For a file reference, we'd extend the parser to
        // give us a callback value with the offset into the file; and we'd make the streaming
        // UTXO parser public and reusable, adding it to the resolver implementation.
        Ok(())
    }
}

#[module(
    message_type(Message),
    name = "snapshot-bootstrapper",
    description = "Snapshot Bootstrapper to broadcast state via streaming"
)]
pub struct SnapshotBootstrapper;

impl SnapshotBootstrapper {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = SnapshotConfig::try_load(&config)?;

        info!("Snapshot bootstrapper initializing");
        info!("  Network: {}", cfg.network);
        info!("  Data directory: {}", cfg.data_dir);
        info!("  Publishing on '{}'", cfg.snapshot_topic);
        info!("  Completing with '{}'", cfg.completion_topic);

        let startup_sub = context.subscribe(&cfg.startup_topic).await?;
        let bootstrapped_sub = context.subscribe(&cfg.bootstrapped_topic).await?;

        context.clone().run(async move {
            let span = info_span!("snapshot_bootstrapper.handle");
            async {
                // Wait for startup signal
                if let Err(e) = Self::wait_startup(startup_sub).await {
                    error!("Failed waiting for startup: {e:#}");
                    return;
                }

                // Wait for genesis bootstrap completion
                if let Err(e) = Self::wait_genesis_completion(bootstrapped_sub).await {
                    error!("Failed waiting for bootstrapped: {e:#}");
                    return;
                }

                info!("Bootstrap prerequisites met, starting snapshot processing");

                // Load network configuration
                let network_config = match Self::read_network_config(&cfg.config_path()) {
                    Ok(config) => config,
                    Err(e) => {
                        error!("Failed to read network config: {e:#}");
                        return;
                    }
                };

                // Load snapshots metadata
                let all_snapshots = match Self::read_snapshots_metadata(&cfg.snapshots_path()) {
                    Ok(snapshots) => snapshots,
                    Err(e) => {
                        error!("Failed to read snapshots metadata: {e:#}");
                        return;
                    }
                };

                // Filter snapshots based on network config
                let target_snapshots = Self::filter_snapshots(&network_config, &all_snapshots);
                if target_snapshots.is_empty() {
                    error!(
                        "No snapshots found for requested epochs: {:?}",
                        network_config.snapshots
                    );
                    return;
                }

                info!("Found {} snapshot(s) to process", target_snapshots.len());

                // Download all snapshots
                if let Err(e) =
                    Self::download_snapshots(&target_snapshots, &cfg.network_dir()).await
                {
                    error!("Failed to download snapshots: {e:#}");
                    return;
                }

                // Process snapshots in order
                if let Err(e) =
                    Self::process_snapshots(&target_snapshots, &cfg, context.clone()).await
                {
                    error!("Failed to process snapshots: {e:#}");
                    return;
                }

                info!("Snapshot bootstrap completed successfully");
            }
            .instrument(span)
            .await;
        });

        Ok(())
    }

    async fn wait_startup(mut subscription: Box<dyn Subscription<Message>>) -> Result<()> {
        let (_, _message) = subscription.read().await?;
        info!("Received startup message");
        Ok(())
    }

    async fn wait_genesis_completion(
        mut subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let (_, message) = subscription.read().await?;
        match message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(_complete))) => {
                info!("Received genesis complete message");
                Ok(())
            }
            msg => bail!("Unexpected message in bootstrapped topic: {msg:?}"),
        }
    }

    fn read_network_config(path: &str) -> Result<NetworkConfig, SnapshotBootstrapError> {
        let path_buf = PathBuf::from(path);
        let content = fs::read_to_string(&path_buf)
            .map_err(|e| SnapshotBootstrapError::ReadNetworkConfig(path_buf.clone(), e))?;

        let config: NetworkConfig = serde_json::from_str(&content)
            .map_err(|e| SnapshotBootstrapError::MalformedNetworkConfig(path_buf, e))?;

        Ok(config)
    }

    fn read_snapshots_metadata(
        path: &str,
    ) -> Result<Vec<SnapshotFileMetadata>, SnapshotBootstrapError> {
        let path_buf = PathBuf::from(path);
        let content = fs::read_to_string(&path_buf)
            .map_err(|e| SnapshotBootstrapError::ReadSnapshotsFile(path_buf.clone(), e))?;

        let snapshots: Vec<SnapshotFileMetadata> = serde_json::from_str(&content)
            .map_err(|e| SnapshotBootstrapError::MalformedSnapshotsFile(path_buf, e))?;

        Ok(snapshots)
    }

    fn filter_snapshots(
        network_config: &NetworkConfig,
        all_snapshots: &[SnapshotFileMetadata],
    ) -> Vec<SnapshotFileMetadata> {
        all_snapshots
            .iter()
            .filter(|s| network_config.snapshots.contains(&s.epoch))
            .cloned()
            .collect()
    }

    async fn download_snapshots(
        snapshots: &[SnapshotFileMetadata],
        network_dir: &str,
    ) -> Result<(), SnapshotBootstrapError> {
        for snapshot_meta in snapshots {
            let filename = format!("{}.cbor", snapshot_meta.point);
            let file_path = format!("{}/{}", network_dir, filename);

            Self::download_snapshot(&snapshot_meta.url, &file_path).await?;
        }
        Ok(())
    }

    async fn download_snapshot(url: &str, output_path: &str) -> Result<(), SnapshotBootstrapError> {
        let path = Path::new(output_path);

        if path.exists() {
            info!("Snapshot already exists, skipping: {}", output_path);
            return Ok(());
        }

        info!("Downloading snapshot from {}", url);

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| SnapshotBootstrapError::CreateDirectory(parent.to_path_buf(), e))?;
        }

        let client = reqwest::Client::new();
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(|e| SnapshotBootstrapError::DownloadError(url.to_string(), e))?;

        if !response.status().is_success() {
            return Err(SnapshotBootstrapError::DownloadInvalidStatusCode(
                url.to_string(),
                response.status(),
            ));
        }

        let tmp_path = path.with_extension("partial");
        let mut file = File::create(&tmp_path).await?;

        let mut compressed_data = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| SnapshotBootstrapError::DownloadError(url.to_string(), e))?
        {
            compressed_data.extend_from_slice(&chunk);
        }

        let cursor = io::Cursor::new(&compressed_data);
        let buffered = BufReader::new(cursor);
        let mut decoder = GzipDecoder::new(buffered);
        tokio::io::copy(&mut decoder, &mut file).await?;

        file.sync_all().await?;
        tokio::fs::rename(&tmp_path, output_path).await?;

        info!("Downloaded snapshot to {}", output_path);
        Ok(())
    }

    async fn process_snapshots(
        snapshots: &[SnapshotFileMetadata],
        cfg: &SnapshotConfig,
        context: Arc<Context<Message>>,
    ) -> Result<()> {
        let mut publisher = SnapshotPublisher::new(
            context,
            cfg.completion_topic.clone(),
            cfg.snapshot_topic.clone(),
        );

        // Publish start once at the beginning
        publisher.publish_start().await?;

        for snapshot_meta in snapshots {
            let filename = format!("{}.cbor", snapshot_meta.point);
            let file_path = format!("{}/{}", cfg.network_dir(), filename);

            info!(
                "Processing snapshot for epoch {} from {}",
                snapshot_meta.epoch, file_path
            );

            Self::parse_snapshot(&file_path, &mut publisher).await?;
        }

        let metadata = publisher
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No metadata received from snapshots"))?;

        let block_info = build_block_info_from_metadata(metadata);
        publisher.publish_completion(block_info).await?;

        Ok(())
    }

    async fn parse_snapshot(file_path: &str, publisher: &mut SnapshotPublisher) -> Result<()> {
        info!("Parsing snapshot: {}", file_path);
        let start = Instant::now();

        let parser = StreamingSnapshotParser::new(file_path);
        parser.parse(publisher)?;

        let duration = start.elapsed();
        info!("Parsed snapshot in {:.2?}", duration);

        Ok(())
    }
}

fn build_block_info_from_metadata(metadata: &SnapshotMetadata) -> BlockInfo {
    BlockInfo {
        status: BlockStatus::Immutable,
        slot: 0,
        number: 0,
        hash: BlockHash::default(),
        epoch: metadata.epoch,
        epoch_slot: 0,
        new_epoch: false,
        timestamp: 0,
        era: Era::Conway,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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

    fn create_fake_snapshot(dir: &Path, point: &str) {
        let snapshot_path = dir.join(format!("{}.cbor", point));
        let mut file = fs::File::create(&snapshot_path).unwrap();
        file.write_all(b"fake snapshot data").unwrap();
    }

    #[test]
    fn test_read_network_config_success() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = create_test_network_config(temp_dir.path(), vec![500, 501]);

        let result = SnapshotBootstrapper::read_network_config(config_path.to_str().unwrap());
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.snapshots, vec![500, 501]);
        assert_eq!(config.points.len(), 1);
    }

    #[test]
    fn test_read_network_config_missing_file() {
        let result = SnapshotBootstrapper::read_network_config("/nonexistent/config.json");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SnapshotBootstrapError::ReadNetworkConfig(_, _)
        ));
    }

    #[test]
    fn test_read_network_config_malformed_json() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(b"{ invalid json }").unwrap();

        let result = SnapshotBootstrapper::read_network_config(config_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SnapshotBootstrapError::MalformedNetworkConfig(_, _)
        ));
    }

    #[test]
    fn test_read_snapshots_metadata_success() {
        let temp_dir = TempDir::new().unwrap();
        let snapshots_path =
            create_test_snapshots_metadata(temp_dir.path(), vec![500, 501], "https://example.com");

        let result =
            SnapshotBootstrapper::read_snapshots_metadata(snapshots_path.to_str().unwrap());
        assert!(result.is_ok());

        let snapshots = result.unwrap();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].epoch, 500);
        assert_eq!(snapshots[1].epoch, 501);
    }

    #[test]
    fn test_read_snapshots_metadata_missing_file() {
        let result = SnapshotBootstrapper::read_snapshots_metadata("/nonexistent/snapshots.json");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SnapshotBootstrapError::ReadSnapshotsFile(_, _)
        ));
    }

    #[test]
    fn test_filter_snapshots() {
        let network_config = NetworkConfig {
            snapshots: vec![500, 502],
            points: vec![],
        };

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

        let filtered = SnapshotBootstrapper::filter_snapshots(&network_config, &all_snapshots);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].epoch, 500);
        assert_eq!(filtered[1].epoch, 502);
    }

    #[tokio::test]
    async fn test_download_snapshot_skips_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let point = "point_500";
        create_fake_snapshot(temp_dir.path(), point);

        let file_path = temp_dir.path().join(format!("{}.cbor", point));

        let result = SnapshotBootstrapper::download_snapshot(
            "https://example.com/snapshot.cbor.gz",
            file_path.to_str().unwrap(),
        )
        .await;

        assert!(result.is_ok());
        assert!(file_path.exists());
    }

    #[tokio::test]
    async fn test_download_snapshot_missing_file_fails() {
        let temp_dir = TempDir::new().unwrap();
        let point = "point_500";
        let file_path = temp_dir.path().join(format!("{}.cbor", point));

        let result = SnapshotBootstrapper::download_snapshot(
            "https://invalid-url-that-does-not-exist.com/snapshot.cbor.gz",
            file_path.to_str().unwrap(),
        )
        .await;

        assert!(result.is_err());
        assert!(!file_path.exists());
    }

    #[test]
    fn test_snapshot_filtering_by_epoch() {
        let temp_dir = TempDir::new().unwrap();
        create_test_network_config(temp_dir.path(), vec![500, 502]);
        create_test_snapshots_metadata(
            temp_dir.path(),
            vec![500, 501, 502, 503],
            "https://example.com",
        );

        let network_config = SnapshotBootstrapper::read_network_config(
            temp_dir.path().join("config.json").to_str().unwrap(),
        )
        .unwrap();

        let all_snapshots = SnapshotBootstrapper::read_snapshots_metadata(
            temp_dir.path().join("snapshots.json").to_str().unwrap(),
        )
        .unwrap();

        let target_snapshots =
            SnapshotBootstrapper::filter_snapshots(&network_config, &all_snapshots);

        assert_eq!(target_snapshots.len(), 2);
        assert_eq!(target_snapshots[0].epoch, 500);
        assert_eq!(target_snapshots[1].epoch, 502);
    }

    #[test]
    fn test_empty_snapshots_list() {
        let temp_dir = TempDir::new().unwrap();
        create_test_network_config(temp_dir.path(), vec![999]);
        create_test_snapshots_metadata(temp_dir.path(), vec![500, 501], "https://example.com");

        let network_config = SnapshotBootstrapper::read_network_config(
            temp_dir.path().join("config.json").to_str().unwrap(),
        )
        .unwrap();

        let all_snapshots = SnapshotBootstrapper::read_snapshots_metadata(
            temp_dir.path().join("snapshots.json").to_str().unwrap(),
        )
        .unwrap();

        let target_snapshots =
            SnapshotBootstrapper::filter_snapshots(&network_config, &all_snapshots);

        assert!(target_snapshots.is_empty());
    }

    #[tokio::test]
    async fn test_download_snapshot_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let nested_path = temp_dir.path().join("nested").join("directory").join("snapshot.cbor");

        let _ = SnapshotBootstrapper::download_snapshot(
            "https://invalid-url.com/snapshot.cbor.gz",
            nested_path.to_str().unwrap(),
        )
        .await;

        assert!(nested_path.parent().unwrap().exists());
    }

    #[test]
    fn test_corrupted_config_json_fails_gracefully() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(b"{\"snapshots\": [500, 501]").unwrap();

        let result = SnapshotBootstrapper::read_network_config(config_path.to_str().unwrap());
        assert!(result.is_err());

        if let Err(SnapshotBootstrapError::MalformedNetworkConfig(path, _)) = result {
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

        let result =
            SnapshotBootstrapper::read_snapshots_metadata(snapshots_path.to_str().unwrap());
        assert!(result.is_err());

        if let Err(SnapshotBootstrapError::MalformedSnapshotsFile(path, _)) = result {
            assert_eq!(path, snapshots_path);
        } else {
            panic!("Expected MalformedSnapshotsFile error");
        }
    }

    #[tokio::test]
    async fn test_download_creates_partial_file_then_renames() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("snapshot.cbor");

        let result = SnapshotBootstrapper::download_snapshot(
            "https://invalid-url.com/snapshot.cbor.gz",
            output_path.to_str().unwrap(),
        )
        .await;

        assert!(result.is_err());
        assert!(!output_path.exists());
    }
}
