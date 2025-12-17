mod block;
mod configuration;
mod context;
mod downloader;
mod nonces;
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

        info!(
            network = %cfg.network,
            data_dir = %cfg.data_dir.display(),
            topic = %cfg.snapshot_topic,
            timeout_secs = cfg.download.timeout_secs,
            connect_timeout_secs = cfg.download.connect_timeout_secs,
            "Initializing"
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

        let total_start = Instant::now();
        let bootstrap_ctx = BootstrapContext::load(&cfg)?;
        info!(
            epoch = bootstrap_ctx.block_info.epoch,
            slot = bootstrap_ctx.block_info.slot,
            block = bootstrap_ctx.block_info.number,
            "Starting bootstrap"
        );

        let download_start = Instant::now();
        let downloader = SnapshotDownloader::new(bootstrap_ctx.network_dir(), &cfg.download)?;
        downloader.download(&bootstrap_ctx.snapshot).await.map_err(BootstrapError::Download)?;
        info!(elapsed = ?download_start.elapsed(), "Snapshot downloaded");

        let mut publisher = SnapshotPublisher::new(
            context,
            cfg.completion_topic.clone(),
            cfg.snapshot_topic.clone(),
            bootstrap_ctx.context(),
        );

        publisher.publish_start().await?;

        let parse_start = Instant::now();
        let parser = StreamingSnapshotParser::new(
            bootstrap_ctx.snapshot_path().to_string_lossy().into_owned(),
        );
        parser
            .parse(&mut publisher, cfg.network.into())
            .map_err(|e| BootstrapError::Parse(e.to_string()))?;
        info!(elapsed = ?parse_start.elapsed(), "Snapshot parsed");

        publisher.publish_snapshot_complete().await?;
        publisher.publish_completion(bootstrap_ctx.block_info).await?;

        info!(elapsed = ?total_start.elapsed(), "Bootstrap complete");
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
