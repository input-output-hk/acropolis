//! Acropolis Mithril snapshot fetcher module for Caryatid
//! Fetches a snapshot from Mithril and replays all the blocks in it

use caryatid_sdk::{Context, Module, module};
use acropolis_messages::{BlockHeaderMessage, BlockBodyMessage, Message};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};

const DEFAULT_HEADER_TOPIC: &str = "cardano.block.header";
const DEFAULT_BODY_TOPIC: &str = "cardano.block.body";

/// Genesis bootstrapper module
#[module(
    message_type(Message),
    name = "mithril-snapshot-fetcher",
    description = "Mithril snapshot fetcher"
)]
pub struct MithrilSnapshotFetcher;

impl MithrilSnapshotFetcher
{
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
      let header_topic = config.get_string("header-topic").unwrap_or(DEFAULT_HEADER_TOPIC.to_string());
      let body_topic = config.get_string("body-topic").unwrap_or(DEFAULT_BODY_TOPIC.to_string());


      Ok(())
    }
}
