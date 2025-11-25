mod configuration;
mod downloader;
mod progress_reader;
mod publisher;

use crate::configuration::{ConfigError, NetworkConfig, SnapshotConfig, SnapshotFileMetadata};
use crate::downloader::{DownloadError, SnapshotDownloader};
use crate::publisher::SnapshotPublisher;
use acropolis_common::snapshot::streaming_snapshot::StreamingSnapshotParser;
use acropolis_common::{
    messages::{CardanoMessage, Message},
    BlockHash, BlockInfo, BlockStatus, Era,
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use thiserror::Error;
use tokio::time::Instant;
use tracing::{error, info, info_span, Instrument};

#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Download error: {0}")]
    Download(#[from] DownloadError),

    #[error("Snapshot parsing failed: {0}")]
    Parse(String),

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
        let cfg = SnapshotConfig::try_load(&config)?;

        info!("Snapshot bootstrapper initializing");
        info!("  Network: {}", cfg.network);
        info!("  Data directory: {}", cfg.data_dir);
        info!("  Publishing on '{}'", cfg.snapshot_topic);
        info!("  Completing with '{}'", cfg.completion_topic);

        let startup_sub = context.subscribe(&cfg.startup_topic).await?;
        let bootstrapped_sub = context.subscribe(&cfg.bootstrapped_subscribe_topic).await?;

        context.clone().run(async move {
            let span = info_span!("snapshot_bootstrapper.handle");
            async {
                // Wait for the startup signal
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

                // Filter snapshots based on network config
                let target_snapshots = SnapshotFileMetadata::filter_by_epochs(
                    &all_snapshots,
                    &network_config.snapshots,
                );
                if target_snapshots.is_empty() {
                    error!(
                        "No snapshots found for requested epochs: {:?}",
                        network_config.snapshots
                    );
                    return;
                }

                info!("Found {} snapshot(s) to process", target_snapshots.len());

                // Create downloader and download all snapshots
                let downloader = match SnapshotDownloader::new(cfg.network_dir()) {
                    Ok(d) => d,
                    Err(e) => {
                        error!("Failed to create snapshot downloader: {e:#}");
                        return;
                    }
                };

                if let Err(e) = downloader.download_all(&target_snapshots).await {
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

        publisher.publish_start().await?;

        for snapshot_meta in snapshots {
            let file_path = snapshot_meta.file_path(&cfg.network_dir());

            info!(
                "Processing snapshot for epoch {} from {}",
                snapshot_meta.epoch, file_path
            );

            Self::parse_snapshot(&file_path, &mut publisher).await?;
        }

        let metadata = publisher
            .metadata()
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

fn build_block_info_from_metadata(
    metadata: &acropolis_common::snapshot::streaming_snapshot::SnapshotMetadata,
) -> BlockInfo {
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
