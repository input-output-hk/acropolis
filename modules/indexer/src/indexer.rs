//! Acropolis indexer module for Caryatid

use acropolis_common::{
    commands::chain_sync::ChainSyncCommand,
    hash::Hash,
    messages::{Command, Message},
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::{str::FromStr, sync::Arc};
use tracing::info;

// Configuration defaults
const DEFAULT_DYNAMIC_SYNC_TOPIC: (&str, &str) =
    ("dynamic-sync-publisher-topic", "cardano.sync.command");

/// Indexer module
#[module(
    message_type(Message),
    name = "indexer",
    description = "Core indexer module for indexer process"
)]
pub struct Indexer;

impl Indexer {
    /// Async initialisation
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let dynamic_sync_publisher_topic = config
            .get_string(DEFAULT_DYNAMIC_SYNC_TOPIC.0)
            .unwrap_or(DEFAULT_DYNAMIC_SYNC_TOPIC.1.to_string());
        info!("Creating dynamic sync publisher on '{dynamic_sync_publisher_topic}'");

        let ctx = context.clone();

        // This is a placeholder to test dynamic sync
        context.run(async move {
            let example = ChainSyncCommand::ChangeSyncPoint {
                slot: 4492799,
                hash: Hash::from_str(
                    "f8084c61b6a238acec985b59310b6ecec49c0ab8352249afd7268da5cff2a457",
                )
                .expect("Valid hash"),
            };

            // Initial sync message (This will be read from config for first sync and from DB on subsequent runs)
            ctx.message_bus
                .publish(
                    &dynamic_sync_publisher_topic,
                    Arc::new(Message::Command(Command::ChainSync(example.clone()))),
                )
                .await
                .unwrap();

            // Simulate a later sync command to reset sync point to where we started
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            ctx.message_bus
                .publish(
                    &dynamic_sync_publisher_topic,
                    Arc::new(Message::Command(Command::ChainSync(example))),
                )
                .await
                .unwrap();
        });
        Ok(())
    }
}
