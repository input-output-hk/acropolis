mod configuration;
mod downloader;
mod progress_reader;
mod publisher;

use crate::configuration::{BootstrapFiles, ConfigError, HeaderFileData, SnapshotConfig};
use crate::downloader::{DownloadError, SnapshotDownloader};
use crate::publisher::{BootstrapContext, SnapshotPublisher};
use acropolis_common::genesis_values::GenesisValues;
use acropolis_common::snapshot::streaming_snapshot::StreamingSnapshotParser;
use acropolis_common::{
    messages::{CardanoMessage, Message},
    BlockHash, BlockInfo, BlockStatus, Era,
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use pallas_primitives::babbage::MintedHeader;
use pallas_primitives::conway::Header;
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

    #[error("Snapshot not found for epoch {0}")]
    SnapshotNotFound(u64),

    #[error("Header decoding failed: {0}")]
    HeaderDecode(String),

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
    /// Initialize the snapshot bootstrapper.
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
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

        let bootstrapped_sub = context.subscribe(&cfg.bootstrapped_subscribe_topic).await?;

        context.clone().run(async move {
            let span = info_span!("snapshot_bootstrapper");
            async {
                if let Err(e) = Self::run_bootstrap(bootstrapped_sub, cfg, context).await {
                    error!("Snapshot bootstrap failed: {e:#}");
                }
            }
            .instrument(span)
            .await;
        });

        Ok(())
    }

    /// Main bootstrap workflow.
    async fn run_bootstrap(
        bootstrapped_sub: Box<dyn Subscription<Message>>,
        cfg: SnapshotConfig,
        context: Arc<Context<Message>>,
    ) -> Result<(), BootstrapError> {
        // Wait for genesis bootstrap completion
        Self::wait_genesis_completion(bootstrapped_sub).await?;
        info!("Bootstrap prerequisites met, starting snapshot processing");

        // Load all bootstrap files
        let bootstrap_files = BootstrapFiles::load(&cfg)?;
        let target_epoch = bootstrap_files.target_epoch();
        info!("Target bootstrap epoch: {}", target_epoch);

        // Get the snapshot metadata
        let target_snapshot = bootstrap_files
            .target_snapshot()
            .ok_or(BootstrapError::SnapshotNotFound(target_epoch))?;

        info!(
            "Found snapshot for epoch {} at point {}",
            target_snapshot.epoch, target_snapshot.point
        );

        // Build nonces from config files
        let nonces = bootstrap_files.build_nonces()?;
        Self::log_nonces(&nonces);

        // Build block info from header
        let block_info = Self::build_block_info(&bootstrap_files, &cfg)?;
        info!(
            "Built block info: slot={}, number={}, hash={}",
            block_info.slot, block_info.number, block_info.hash
        );

        // Build bootstrap context with nonces and timing
        // TODO: Make genesis configurable based on network
        let genesis = GenesisValues::mainnet();
        let bootstrap_context = BootstrapContext::new(
            nonces,
            block_info.slot,
            block_info.number,
            block_info.epoch,
            &genesis,
        );

        // Download snapshot if needed
        let downloader = SnapshotDownloader::new(cfg.network_dir(), &cfg.download)?;
        downloader.download(&target_snapshot).await?;

        // Process the snapshot with full context
        let file_path = target_snapshot.file_path(&cfg.network_dir());
        Self::process_snapshot(&file_path, block_info, bootstrap_context, &cfg, context).await?;

        info!("Snapshot bootstrap completed successfully");
        Ok(())
    }

    /// Wait for the genesis bootstrap to complete.
    async fn wait_genesis_completion(
        mut subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let (_, message) = subscription.read().await?;
        match message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(_))) => {
                info!("Received genesis complete message");
                Ok(())
            }
            msg => bail!("Unexpected message in bootstrapped topic: {msg:?}"),
        }
    }

    /// Log nonces for debugging.
    fn log_nonces(nonces: &configuration::NoncesData) {
        let (active, evolving, candidate, tail) = nonces.to_hex_strings();
        info!("Built nonces for epoch {}", nonces.epoch);
        info!("  Active:    {}...", &active[..16]);
        info!("  Evolving:  {}...", &evolving[..16]);
        info!("  Candidate: {}...", &candidate[..16]);
        info!("  Tail:      {}...", &tail[..16]);
    }

    /// Build BlockInfo from bootstrap files.
    fn build_block_info(
        bootstrap_files: &BootstrapFiles,
        _cfg: &SnapshotConfig,
    ) -> Result<BlockInfo, BootstrapError> {
        let epoch = bootstrap_files.target_epoch();
        let header_data = &bootstrap_files.target_header;

        // Decode header to get block height
        let header = Self::extract_block_header(header_data)?;

        // Get hash from header data
        let hash_bytes = header_data.hash_bytes()?;
        let hash = BlockHash::try_from(hash_bytes.to_vec())
            .map_err(|e| BootstrapError::Parse(format!("Invalid block hash: {:?}", e)))?;

        // Calculate epoch slot and timestamp
        // TODO: Make genesis configurable based on network
        let genesis = GenesisValues::mainnet();
        let epoch_start_slot = genesis.epoch_to_first_slot(epoch);
        let timestamp = genesis.slot_to_timestamp(header_data.slot);
        let block_height = header.header_body.block_number;

        info!(
            "Building BlockInfo: slot={}, height={}, epoch={}, epoch_slot={}",
            header_data.slot, block_height, epoch, epoch_start_slot
        );

        Ok(BlockInfo {
            status: BlockStatus::Immutable,
            slot: header_data.slot,
            number: block_height,
            hash,
            epoch,
            epoch_slot: epoch_start_slot,
            new_epoch: true,
            timestamp,
            era: Era::Conway,
        })
    }

    /// Extract block header from CBOR bytes.
    fn extract_block_header(header_data: &HeaderFileData) -> Result<Header, BootstrapError> {
        let minted_header: MintedHeader<'_> =
            minicbor::decode(&header_data.cbor_bytes).map_err(|e| {
                BootstrapError::HeaderDecode(format!(
                    "Failed to decode header at slot {}: {}",
                    header_data.slot, e
                ))
            })?;

        let header = Header::from(minted_header);
        Ok(header)
    }

    /// Process a snapshot file with bootstrap context.
    async fn process_snapshot(
        file_path: &str,
        block_info: BlockInfo,
        bootstrap_context: BootstrapContext,
        cfg: &SnapshotConfig,
        context: Arc<Context<Message>>,
    ) -> Result<(), BootstrapError> {
        // Create publisher with bootstrap context for nonces/timing
        let mut publisher = SnapshotPublisher::new(
            context,
            cfg.completion_topic.clone(),
            cfg.snapshot_topic.clone(),
        )
        .with_bootstrap_context(bootstrap_context);

        publisher.publish_start().await?;

        info!("Processing snapshot from {}", file_path);
        Self::parse_snapshot(file_path, &mut publisher).await?;

        publisher.publish_completion(block_info).await?;

        Ok(())
    }

    /// Parse a snapshot file using the streaming parser.
    async fn parse_snapshot(
        file_path: &str,
        publisher: &mut SnapshotPublisher,
    ) -> Result<(), BootstrapError> {
        info!("Parsing snapshot: {}", file_path);
        let start = Instant::now();

        let parser = StreamingSnapshotParser::new(file_path);
        parser.parse(publisher).map_err(|e| BootstrapError::Parse(e.to_string()))?;

        info!("Parsed snapshot in {:.2?}", start.elapsed());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_error_display() {
        let err = BootstrapError::SnapshotNotFound(509);
        assert_eq!(err.to_string(), "Snapshot not found for epoch 509");

        let err = BootstrapError::HeaderDecode("test error".to_string());
        assert_eq!(err.to_string(), "Header decoding failed: test error");
    }
}
