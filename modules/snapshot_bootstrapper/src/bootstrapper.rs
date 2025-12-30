mod block;
mod configuration;
mod context;
mod downloader;
mod nonces;
pub(crate) mod opcerts;
mod progress_reader;
mod publisher;

use crate::configuration::BootstrapConfig;
use crate::context::{BootstrapContext, BootstrapContextError};
use crate::downloader::{DownloadError, SnapshotDownloader};
use crate::publisher::SnapshotPublisher;
use acropolis_common::{
    configuration::StartupMethod,
    messages::{CardanoMessage, Message},
    snapshot::streaming_snapshot::StreamingSnapshotParser,
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
    Data(#[from] BootstrapContextError),

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
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Check if this module is the selected startup method
        let startup_method = StartupMethod::from_config(&config);
        if !startup_method.is_snapshot() {
            info!(
                "Snapshot bootstrapper not enabled (startup.method = '{}')",
                startup_method
            );
            return Ok(());
        }

        let cfg = BootstrapConfig::try_load(&config)?;

        info!("Snapshot bootstrapper initializing");
        info!("  Network: {}", cfg.network);
        info!("  Data directory: {}", cfg.data_dir.display());
        info!("  Publishing on '{}'", cfg.snapshot_topic);
        info!(
            "  Download timeouts: {}s total, {}s connect",
            cfg.download.timeout_secs, cfg.download.connect_timeout_secs
        );

        let bootstrapped_sub = context.subscribe(&cfg.bootstrapped_subscribe_topic).await?;

        context.clone().run(async move {
            let span = info_span!("snapshot_bootstrapper");
            async {
                if let Err(e) = Self::run(bootstrapped_sub, cfg, context).await {
                    error!("Snapshot bootstrap failed: {e:#}");
                }
            }
            .instrument(span)
            .await;
        });

        Ok(())
    }

    async fn run(
        bootstrapped_sub: Box<dyn Subscription<Message>>,
        cfg: BootstrapConfig,
        context: Arc<Context<Message>>,
    ) -> Result<(), BootstrapError> {
        Self::wait_for_genesis(bootstrapped_sub).await?;

        let bootstrap_ctx = BootstrapContext::load(&cfg)?;
        info!(
            "Loaded bootstrap data for epoch {}",
            bootstrap_ctx.block_info.epoch
        );
        info!("  Snapshot: {}", bootstrap_ctx.snapshot.url);
        info!(
            "  Block: slot={}, number={}",
            bootstrap_ctx.block_info.slot, bootstrap_ctx.block_info.number
        );
        info!(
            "  OpCert counters: {} pools",
            bootstrap_ctx.ocert_counters.len()
        );

        // Publish
        let mut publisher = SnapshotPublisher::new(
            context.clone(),
            cfg.snapshot_topic.clone(),
            cfg.sync_command_topic.clone(),
            bootstrap_ctx.context(),
        );
        // Download
        let downloader = SnapshotDownloader::new(bootstrap_ctx.network_dir(), &cfg.download)?;
        downloader.download(&bootstrap_ctx.snapshot).await.map_err(BootstrapError::Download)?;

        publisher.publish_start().await?;

        info!(
            "Parsing snapshot: {}",
            bootstrap_ctx.snapshot_path().display()
        );
        let start = Instant::now();
        let parser = StreamingSnapshotParser::new(
            bootstrap_ctx.snapshot_path().to_string_lossy().into_owned(),
        );
        parser
            .parse(&mut publisher, cfg.network.into())
            .map_err(|e| BootstrapError::Parse(e.to_string()))?;
        info!("Parsed snapshot in {:.2?}", start.elapsed());

        // Publish KES validator bootstrap data (opcert counters from CSV)
        publisher
            .publish_kes_validator_bootstrap(
                bootstrap_ctx.block_info.epoch,
                bootstrap_ctx.ocert_counters,
            )
            .await?;

        publisher.publish_snapshot_complete().await?;
        publisher.start_chain_sync(bootstrap_ctx.block_info.to_point()).await?;

        info!("Snapshot bootstrap completed");
        Ok(())
    }

    async fn wait_for_genesis(mut sub: Box<dyn Subscription<Message>>) -> Result<()> {
        let (_, msg) = sub.read().await?;
        match msg.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(_))) => {
                info!("Genesis complete, starting snapshot bootstrap");
                Ok(())
            }
            other => bail!("Unexpected message: {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_error_display() {
        let err = BootstrapError::Parse("test".to_string());
        assert_eq!(err.to_string(), "Snapshot parsing failed: test");
    }
}
