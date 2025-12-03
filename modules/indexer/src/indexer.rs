//! Acropolis indexer module for Caryatid
mod configuration;

use acropolis_common::{
    commands::chain_sync::ChainSyncCommand,
    hash::Hash,
    messages::{Command, Message},
    Point,
};
use anyhow::Result;
use caryatid_sdk::{module, Context};
use config::Config;
use std::{str::FromStr, sync::Arc};
use tracing::info;

use crate::configuration::IndexerConfig;

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
        let cfg = IndexerConfig::try_load(&config)?;
        info!(
            "Creating sync command publisher on '{}'",
            cfg.sync_command_topic
        );

        let ctx = context.clone();

        // This is a placeholder to test dynamic sync
        context.run(async move {
            let example = ChainSyncCommand::FindIntersect(Point::Specific {
                slot: 4492799,
                hash: Hash::from_str(
                    "f8084c61b6a238acec985b59310b6ecec49c0ab8352249afd7268da5cff2a457",
                )
                .expect("Valid hash"),
            });

            // Initial sync message (This will be read from config for first sync and from DB on subsequent runs)
            ctx.message_bus
                .publish(
                    &cfg.sync_command_topic,
                    Arc::new(Message::Command(Command::ChainSync(example.clone()))),
                )
                .await
                .unwrap();

            // Simulate a later sync command to reset sync point to where we started

            ctx.message_bus
                .publish(
                    &cfg.sync_command_topic,
                    Arc::new(Message::Command(Command::ChainSync(example))),
                )
                .await
                .unwrap();
        });
        Ok(())
    }
}
