use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use acropolis_common::{
    genesis_values::GenesisValues,
    hash::Hash,
    messages::{CardanoMessage, GenesisCompleteMessage, Message},
    snapshot::{
        streaming_snapshot::{
            DRepCallback, DRepInfo, GovernanceProposal, PoolCallback, PoolInfo, ProposalCallback,
            SnapshotCallbacks, SnapshotMetadata, StakeCallback, UtxoCallback, UtxoEntry,
        },
        StreamingSnapshotParser,
    },
    stake_addresses::AccountState,
    BlockHash, BlockInfo, BlockStatus, Era, GenesisDelegates,
};
use anyhow::Result;
use async_compression::tokio::bufread::GzipDecoder;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::BufReader;
use tokio::time::Instant;
use tokio_util::io::StreamReader;
use tracing::{error, info, info_span, Instrument};

const DEFAULT_SNAPSHOT_TOPIC: &str = "cardano.snapshot";
const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.start";
const DEFAULT_COMPLETION_TOPIC: &str = "cardano.sequence.bootstrapped";

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

/// Callback handler that accumulates snapshot data and builds state
pub struct SnapshotHandler {
    context: Arc<Context<Message>>,
    snapshot_topic: String,

    // Accumulated data from callbacks
    metadata: Option<SnapshotMetadata>,
    utxo_count: u64,
    pools: Vec<PoolInfo>,
    accounts: Vec<AccountState>,
    dreps: Vec<DRepInfo>,
    proposals: Vec<GovernanceProposal>,
}

#[module(
    message_type(Message),
    name = "snapshot-bootstrapper",
    description = "Snapshot Bootstrapper to broadcast state"
)]
pub struct SnapshotBootstrapper;

impl SnapshotHandler {
    fn new(context: Arc<Context<Message>>, snapshot_topic: String) -> Self {
        Self {
            context,
            snapshot_topic,
            metadata: None,
            utxo_count: 0,
            pools: Vec::new(),
            accounts: Vec::new(),
            dreps: Vec::new(),
            proposals: Vec::new(),
        }
    }

    /// Build BlockInfo from accumulated metadata
    fn build_block_info(&self) -> Result<BlockInfo> {
        let metadata =
            self.metadata.as_ref().ok_or_else(|| anyhow::anyhow!("No metadata available"))?;

        Ok(BlockInfo {
            status: BlockStatus::Immutable, // Snapshot blocks are immutable
            slot: 0,                        // TODO: Extract from snapshot metadata if available
            number: 0,                      // TODO: Extract from snapshot metadata if available
            hash: BlockHash::default(),     // TODO: Extract from snapshot metadata if available
            epoch: metadata.epoch,
            epoch_slot: 0,    // TODO: Extract from snapshot metadata if available
            new_epoch: false, // Not necessarily a new epoch
            timestamp: 0,     // TODO: Extract from snapshot metadata if available
            era: Era::Conway, // TODO: Determine from snapshot or config
        })
    }

    /// Build GenesisValues from snapshot data
    fn build_genesis_values(&self) -> Result<GenesisValues> {
        // TODO: These values should ideally come from the snapshot or configuration
        // For now, using defaults for Conway era
        Ok(GenesisValues {
            byron_timestamp: 1506203091, // Byron mainnet genesis timestamp
            shelley_epoch: 208,          // Shelley started at epoch 208 on mainnet
            shelley_epoch_len: 432000,   // 5 days in seconds
            // Shelley mainnet genesis hash (placeholder - should be from config)
            shelley_genesis_hash: Hash::<32>::from_str(
                "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81",
            )?,
            genesis_delegs: GenesisDelegates::try_from(vec![]).unwrap(),
        })
    }

    async fn publish_start(&self) -> Result<()> {
        self.context
            .message_bus
            .publish(
                &self.snapshot_topic,
                Arc::new(Message::Snapshot(
                    acropolis_common::messages::SnapshotMessage::Startup,
                )),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to publish start message: {}", e))
    }

    async fn publish_completion(
        &self,
        block_info: BlockInfo,
        genesis_values: GenesisValues,
    ) -> Result<()> {
        let message = Message::Cardano((
            block_info,
            CardanoMessage::GenesisComplete(GenesisCompleteMessage {
                values: genesis_values,
            }),
        ));

        self.context
            .message_bus
            .publish(&self.snapshot_topic, Arc::new(message))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to publish completion: {}", e))
    }
}

impl UtxoCallback for SnapshotHandler {
    fn on_utxo(&mut self, _utxo: UtxoEntry) -> Result<()> {
        self.utxo_count += 1;
        if self.utxo_count.is_multiple_of(1_000_000) {
            info!("Processed {} UTXOs", self.utxo_count);
        }
        Ok(())
    }
}

impl PoolCallback for SnapshotHandler {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        info!("Received {} pools", pools.len());
        self.pools.extend(pools);
        Ok(())
    }
}

impl StakeCallback for SnapshotHandler {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        info!("Received {} accounts", accounts.len());
        self.accounts.extend(accounts);
        Ok(())
    }
}

impl DRepCallback for SnapshotHandler {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()> {
        info!("Received {} DReps", dreps.len());
        self.dreps.extend(dreps);
        Ok(())
    }
}

impl ProposalCallback for SnapshotHandler {
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()> {
        info!("Received {} proposals", proposals.len());
        self.proposals.extend(proposals);
        Ok(())
    }
}

impl SnapshotCallbacks for SnapshotHandler {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        info!("Received snapshot metadata for epoch {}", metadata.epoch);
        info!("  - UTXOs: {:?}", metadata.utxo_count);
        info!(
            "  - Pot balances: treasury={}, reserves={}, deposits={}",
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
        Ok(())
    }
}

impl SnapshotBootstrapper {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let network = config.get_string("network").unwrap_or_else(|_| "mainnet".to_string());
        let data_dir = config.get_string("data-dir").unwrap_or_else(|_| "./data".to_string());
        let startup_topic =
            config.get_string("startup-topic").unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());
        let snapshot_topic =
            config.get_string("snapshot-topic").unwrap_or(DEFAULT_SNAPSHOT_TOPIC.to_string());
        let completion_topic =
            config.get_string("completion-topic").unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string());

        info!("Snapshot bootstrapper initializing");
        info!("  Network: {}", network);
        info!("  Data directory: {}", data_dir);
        info!("  Publishing on '{}'", snapshot_topic);

        let mut subscription = context.subscribe(&startup_topic).await?;

        context.clone().run(async move {
            let Ok(_) = subscription.read().await else {
                return;
            };
            info!("Received startup signal");

            let span = info_span!("snapshot_bootstrapper.handle");
            async {
                let network_dir = format!("{}/{}", data_dir, network);
                let config_path = format!("{}/config.json", network_dir);
                let snapshots_path = format!("{}/snapshots.json", network_dir);

                let network_config = match Self::read_network_config(&config_path) {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        error!("Failed to read network config: {}", e);
                        return;
                    }
                };

                info!(
                    "Loading snapshots for epochs: {:?}",
                    network_config.snapshots
                );

                let all_snapshots = match Self::read_snapshots_metadata(&snapshots_path) {
                    Ok(snaps) => snaps,
                    Err(e) => {
                        error!("Failed to read snapshots metadata: {}", e);
                        return;
                    }
                };

                let target_snapshots: Vec<_> = all_snapshots
                    .iter()
                    .filter(|s| network_config.snapshots.contains(&s.epoch))
                    .cloned()
                    .collect();

                if target_snapshots.is_empty() {
                    error!(
                        "No snapshots found for requested epochs: {:?}",
                        network_config.snapshots
                    );
                    return;
                }

                info!("Found {} snapshot files to process", target_snapshots.len());

                for snapshot_meta in &target_snapshots {
                    let filename = format!("{}.cbor", snapshot_meta.point);
                    let file_path = format!("{}/{}", network_dir, filename);

                    if let Err(e) =
                        Self::ensure_snapshot_downloaded(&file_path, snapshot_meta).await
                    {
                        error!("Failed to download snapshot: {}", e);
                        return;
                    }
                }

                for snapshot_meta in target_snapshots {
                    let filename = format!("{}.cbor", snapshot_meta.point);
                    let file_path = format!("{}/{}", network_dir, filename);

                    info!(
                        "Processing snapshot for epoch {} from {}",
                        snapshot_meta.epoch, file_path
                    );

                    if let Err(e) =
                        Self::process_snapshot(&file_path, context.clone(), &completion_topic).await
                    {
                        error!("Failed to process snapshot: {}", e);
                        return;
                    }
                }

                info!("Snapshot bootstrap completed successfully");
            }
            .instrument(span)
            .await;
        });

        Ok(())
    }

    /// Read network configuration
    fn read_network_config(path: &str) -> Result<NetworkConfig, SnapshotBootstrapError> {
        let path_buf = PathBuf::from(path);
        let content = fs::read_to_string(&path_buf)
            .map_err(|e| SnapshotBootstrapError::ReadNetworkConfig(path_buf.clone(), e))?;

        let config: NetworkConfig = serde_json::from_str(&content)
            .map_err(|e| SnapshotBootstrapError::MalformedNetworkConfig(path_buf, e))?;

        Ok(config)
    }

    /// Read snapshot metadata
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

    /// Ensure the snapshot is downloaded
    async fn ensure_snapshot_downloaded(
        file_path: &str,
        metadata: &SnapshotFileMetadata,
    ) -> Result<(), SnapshotBootstrapError> {
        let path = Path::new(file_path);

        if path.exists() {
            info!("Snapshot file already exists: {}", file_path);
            return Ok(());
        }

        info!(
            "Downloading snapshot from {} to {}",
            metadata.url, file_path
        );
        Self::download_snapshot(&metadata.url, file_path).await?;
        info!("Downloaded: {}", file_path);
        Ok(())
    }

    async fn download_snapshot(url: &str, output_path: &str) -> Result<(), SnapshotBootstrapError> {
        if let Some(parent) = Path::new(output_path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| SnapshotBootstrapError::CreateDirectory(parent.to_path_buf(), e))?;
        }

        let client = reqwest::Client::new();
        let response = client
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

        let total_size = response.content_length().unwrap_or(0);
        if total_size > 0 {
            info!("Downloading {} MB (compressed)...", total_size / 1_000_000);
        }

        let tmp_path = Path::new(output_path).with_extension("partial");
        let mut file = File::create(&tmp_path).await?;

        let raw_stream_reader =
            StreamReader::new(response.bytes_stream().map_err(io::Error::other));
        let buffered_reader = BufReader::new(raw_stream_reader);
        let mut decoded_stream = GzipDecoder::new(buffered_reader);

        tokio::io::copy(&mut decoded_stream, &mut file).await?;
        file.sync_all().await?;
        tokio::fs::rename(&tmp_path, output_path).await?;

        Ok(())
    }

    /// Process a single snapshot file
    async fn process_snapshot(
        file_path: &str,
        context: Arc<Context<Message>>,
        completion_topic: &str,
    ) -> Result<()> {
        let parser = StreamingSnapshotParser::new(file_path);
        let mut callbacks = SnapshotHandler::new(context.clone(), completion_topic.to_string());

        info!("Starting snapshot parsing: {}", file_path);
        let start = Instant::now();

        callbacks.publish_start().await?;
        parser.parse(&mut callbacks)?;

        let duration = start.elapsed();
        info!("Parsed snapshot in {:.2?}", duration);

        let block_info = callbacks.build_block_info()?;
        let genesis_values = callbacks.build_genesis_values()?;

        callbacks.publish_completion(block_info, genesis_values).await?;

        Ok(())
    }
}
