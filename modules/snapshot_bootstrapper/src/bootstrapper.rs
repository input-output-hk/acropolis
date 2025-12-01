mod configuration;
mod data;
mod downloader;
mod header;
mod nonces;
mod progress_reader;
mod publisher;

use crate::configuration::BootstrapConfig;
use crate::data::{BootstrapData, BootstrapDataError};
use crate::downloader::{DownloadError, SnapshotDownloader};
use crate::publisher::SnapshotPublisher;
use acropolis_common::messages::{CardanoMessage, Message};
use acropolis_common::snapshot::streaming_snapshot::StreamingSnapshotParser;
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
    Data(#[from] BootstrapDataError),

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
        let cfg = BootstrapConfig::try_load(&config)?;

        info!("Snapshot bootstrapper initializing");
        info!("  Network: {}", cfg.network);
        info!("  Data directory: {}", cfg.data_dir);
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

        let data = BootstrapData::load(&cfg)?;
        info!("Loaded bootstrap data for epoch {}", data.block_info.epoch);
        info!("  Snapshot: {}", data.snapshot.url);
        info!(
            "  Block: slot={}, height={}",
            data.block_info.slot, data.block_info.number
        );

        // Download
        let downloader = SnapshotDownloader::new(data.network_dir(), &cfg.download)?;
        downloader.download(&data.snapshot).await?;

        // Publish
        let mut publisher = SnapshotPublisher::new(
            context,
            cfg.completion_topic.clone(),
            cfg.snapshot_topic.clone(),
        )
        .with_bootstrap_context(data.context());

        publisher.publish_start().await?;

        info!("Parsing snapshot: {}", data.snapshot_path());
        let start = Instant::now();
        let parser = StreamingSnapshotParser::new(data.snapshot_path());
        parser.parse(&mut publisher).map_err(|e| BootstrapError::Parse(e.to_string()))?;
        info!("Parsed snapshot in {:.2?}", start.elapsed());

        publisher.publish_completion(data.block_info).await?;

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
