mod configuration;
mod downloader;
mod progress_reader;
mod publisher;

use crate::configuration::{ConfigError, NetworkConfig, SnapshotConfig, SnapshotFileMetadata};
use crate::downloader::{DownloadError, SnapshotDownloader};
use crate::publisher::SnapshotPublisher;
use acropolis_common::genesis_values::GenesisValues;
use acropolis_common::snapshot::streaming_snapshot::StreamingSnapshotParser;
use acropolis_common::{
    messages::{CardanoMessage, Message},
    BlockHash, BlockInfo, BlockStatus, Era, StartupMethod,
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use thiserror::Error;
use tokio::time::Instant;
use tracing::{error, info, info_span, Instrument};

const CONFIG_KEY_START_UP_METHOD: &str = "startup-method";

#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Download error: {0}")]
    Download(#[from] DownloadError),

    #[error("Snapshot parsing failed: {0}")]
    Parse(String),

    #[error("Snapshot not found for epoch {0}")]
    SnapshotNotFound(u64),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[module(
    message_type(Message),
    name = "snapshot-bootstrapper",
    description = "Snapshot Bootstrapper to broadcast state via streaming"
)]
pub struct SnapshotBootstrapper;

impl SnapshotBootstrapper {
    /// Initializes the snapshot bootstrapper.
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        info!("Config {}", format!("{:?}", config));
        // Check if this module is the selected startup method
        let startup_method = config
            .get::<StartupMethod>(CONFIG_KEY_START_UP_METHOD)
            .unwrap_or(StartupMethod::Mithril);

        if startup_method != StartupMethod::Snapshot {
            info!(
                "Snapshot bootstrapper not enabled (startup.method = '{}')",
                startup_method
            );
            return Ok(());
        }

        let cfg = SnapshotConfig::try_load(&config)?;

        info!("Snapshot bootstrapper initializing");
        info!("  Network: {}", cfg.network);
        info!("  Data directory: {}", cfg.data_dir);
        info!("  Publishing on '{}'", cfg.snapshot_topic);
        info!("  Completing with '{}'", cfg.completion_topic);
        info!(
            "  Download timeouts: {}s total, {}s connect",
            cfg.download.timeout_secs, cfg.download.connect_timeout_secs
        );
        info!(
            "  Progress log interval: {} chunks",
            cfg.download.progress_log_interval
        );

        let bootstrapped_sub = context.subscribe(&cfg.bootstrapped_subscribe_topic).await?;

        context.clone().run(async move {
            let span = info_span!("snapshot_bootstrapper.handle");
            async {
                // Wait for genesis bootstrap completion
                if let Err(e) = Self::wait_genesis_completion(bootstrapped_sub).await {
                    error!("Failed waiting for bootstrapped: {e:#}");
                    return;
                }

                info!("Bootstrap prerequisites met, starting snapshot processing");

                // Load network configuration
                let network_config = match NetworkConfig::read_from_file(&cfg.config_path()) {
                    Ok(config) => config,
                    Err(e) => {
                        error!("Failed to read network config: {e:#}");
                        return;
                    }
                };

                // Load snapshots metadata
                let all_snapshots =
                    match SnapshotFileMetadata::read_all_from_file(&cfg.snapshots_path()) {
                        Ok(snapshots) => snapshots,
                        Err(e) => {
                            error!("Failed to read snapshots metadata: {e:#}");
                            return;
                        }
                    };

                // Find the target snapshot based on network config
                let target_snapshot = match SnapshotFileMetadata::find_by_epoch(
                    &all_snapshots,
                    network_config.snapshot,
                ) {
                    Some(snapshot) => snapshot,
                    None => {
                        error!(
                            "No snapshot found for requested epoch: {}",
                            network_config.snapshot
                        );
                        return;
                    }
                };

                info!(
                    "Found snapshot for epoch {} at point {}",
                    target_snapshot.epoch, target_snapshot.point
                );

                // Create downloader and download the snapshot
                let downloader = match SnapshotDownloader::new(cfg.network_dir(), &cfg.download) {
                    Ok(d) => d,
                    Err(e) => {
                        error!("Failed to create snapshot downloader: {e:#}");
                        return;
                    }
                };

                let file_path = match downloader.download(&target_snapshot).await {
                    Ok(path) => path,
                    Err(e) => {
                        error!("Failed to download snapshot: {e:#}");
                        return;
                    }
                };

                // Process the snapshot
                if let Err(e) =
                    Self::process_snapshot(&target_snapshot, &file_path, &cfg, context.clone())
                        .await
                {
                    error!("Failed to process snapshot: {e:#}");
                    return;
                }

                info!("Snapshot bootstrap completed successfully");
            }
            .instrument(span)
            .await;
        });

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

    async fn process_snapshot(
        snapshot_meta: &SnapshotFileMetadata,
        file_path: &str,
        cfg: &SnapshotConfig,
        context: Arc<Context<Message>>,
    ) -> Result<()> {
        let mut publisher = SnapshotPublisher::new(
            context,
            cfg.completion_topic.clone(),
            cfg.snapshot_topic.clone(),
        );

        publisher.publish_start().await?;

        info!(
            "Processing snapshot for epoch {} from {}",
            snapshot_meta.epoch, file_path
        );

        Self::parse_snapshot(file_path, &mut publisher).await?;

        let block_info = build_block_info_from_metadata(snapshot_meta).map_err(|e| {
            BootstrapError::Parse(format!(
                "Failed to build block info from snapshot metadata: {e}"
            ))
        })?;

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

fn build_block_info_from_metadata(metadata: &SnapshotFileMetadata) -> Result<BlockInfo> {
    let (slot, block_hash_str) = metadata
        .parse_point()
        .ok_or_else(|| anyhow::anyhow!("Invalid point format: {}", metadata.point))?;

    let hash = BlockHash::try_from(hex::decode(block_hash_str)?)
        .map_err(|e| anyhow::anyhow!("Invalid block hash hex: {:?}", e))?;

    let genesis = GenesisValues::mainnet();
    let epoch_slot = slot - genesis.epoch_to_first_slot(slot);
    let timestamp = genesis.slot_to_timestamp(slot);

    info!(
        "Block info built: slot={}, hash={}, epoch={}, slot_in_epoch={}, timestamp={}",
        slot, hash, metadata.epoch, epoch_slot, timestamp
    );

    Ok(BlockInfo {
        status: BlockStatus::Immutable,
        slot,
        number: 0,
        hash,
        epoch: metadata.epoch,
        epoch_slot,
        new_epoch: false,
        timestamp,
        era: Era::Conway,
    })
}
