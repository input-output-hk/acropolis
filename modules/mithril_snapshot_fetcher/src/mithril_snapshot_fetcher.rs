//! Acropolis Mithril snapshot fetcher module for Caryatid
//! Fetches a snapshot from Mithril and replays all the blocks in it

use caryatid_sdk::{Context, Module, module};
use acropolis_messages::{BlockHeaderMessage, BlockBodyMessage, Message};
use std::sync::Arc;
use anyhow::{Result, anyhow};
use config::Config;
use tracing::{debug, info, error};
use mithril_client::{ClientBuilder, MessageBuilder};
use std::path::Path;

const DEFAULT_HEADER_TOPIC: &str = "cardano.block.header";
const DEFAULT_BODY_TOPIC: &str = "cardano.block.body";
const DEFAULT_AGGREGATOR_URL: &str = "https://aggregator.release-mainnet.api.mithril.network/aggregator";
const DEFAULT_GENESIS_KEY: &str = "5b3139312c36362c3134302c3138352c3133382c31312c3233372c3230372c3235302c3134342c32372c322c3138382c33302c31322c38312c3135352c3230342c31302c3137392c37352c32332c3133382c3139362c3231372c352c31342c32302c35372c37392c33392c3137365d";

/// Mithril snapshot fetcher module
#[module(
    message_type(Message),
    name = "mithril-snapshot-fetcher",
    description = "Mithril snapshot fetcher"
)]
pub struct MithrilSnapshotFetcher;

impl MithrilSnapshotFetcher
{
    /// Fetch and unpack a snapshot
    async fn process_snapshot(context: Arc<Context<Message>>,
                              config: Arc<Config>) -> Result<()> {
        let header_topic = config.get_string("header-topic").
            unwrap_or(DEFAULT_HEADER_TOPIC.to_string());
        let body_topic = config.get_string("body-topic").
            unwrap_or(DEFAULT_BODY_TOPIC.to_string());
        let aggregator_url = config.get_string("aggregator-url").
            unwrap_or(DEFAULT_AGGREGATOR_URL.to_string());
        let genesis_key = config.get_string("genesis-key").
            unwrap_or(DEFAULT_GENESIS_KEY.to_string());

        let client = ClientBuilder::aggregator(&aggregator_url, &genesis_key)
            .build()?;

        let snapshots = client.snapshot().list().await?;
        let latest_snapshot = snapshots.first()
            .ok_or(anyhow!("No snapshots available"))?;
        let snapshot = client.snapshot().get(&latest_snapshot.digest)
            .await?
            .ok_or(anyhow!("No snapshot for digest {}",
                           latest_snapshot.digest))?;
        info!("Using Mithril snapshot {snapshot:?}");

        client.certificate().verify_chain(&snapshot.certificate_hash)
            .await?;

        // TODO Download and verify
        // TODO scan using hardano and output blocks

        Ok(())
    }

    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        tokio::spawn(async move {
            match Self::process_snapshot(context, config).await {
                Err(e) => error!("Failed to use Mithril snapshot: {e}"),
                _ => {}
            }
        });

        Ok(())
    }
}
