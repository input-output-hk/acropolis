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
use acropolis_common::messages::RawBlockMessage;
use acropolis_common::{
    configuration::StartupMethod,
    messages::{CardanoMessage, Message},
    snapshot::streaming_snapshot::StreamingSnapshotParser,
    BlockHash, BlockInfo, BlockIntent, BlockStatus,
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use thiserror::Error;
use tokio::time::Instant;
use tracing::{error, info, info_span, Instrument};

const DEFAULT_BLOCK_PUBLISH_TOPIC: (&str, &str) =
    ("block-publish-topic", "cardano.block.available");

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

        // Publish
        let mut publisher = SnapshotPublisher::new(
            context.clone(),
            cfg.completion_topic.clone(),
            cfg.snapshot_topic.clone(),
            bootstrap_ctx.context(),
        );
/*
        // Send Conway era "genesis" block before updating from snapshot
        // because some of the models, like protocol parameters, depend
        // on having a base state. This is synchronization of modules.
        if bootstrap_ctx.block_info.era == acropolis_common::Era::Conway {
            let raw_block = vec![]; // Genesis block has no body
            let header = vec![]; // Genesis block has no header

            // Send the block message
            let message = RawBlockMessage {
                header,
                body: raw_block,
            };

            let block_info = bootstrap_ctx.block_info.clone();
            let message_enum =
                Message::Cardano((block_info, CardanoMessage::BlockAvailable(message)));

            let block_publish_topic = DEFAULT_BLOCK_PUBLISH_TOPIC.1.to_string();
            info!(
                "Publishing Conway genesis block with blockinfo {:?} to {}",
                bootstrap_ctx.block_info, block_publish_topic
            );

            context
                .clone()
                .message_bus
                .publish(&block_publish_topic, Arc::new(message_enum))
                .await
                .unwrap_or_else(|e| error!("Failed to publish block message: {e}"));
        }
*/
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

        publisher.publish_completion(bootstrap_ctx.block_info).await?;

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
