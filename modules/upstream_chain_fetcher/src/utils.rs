use acropolis_common::messages::{CardanoMessage, Message};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::Context;
use config::Config;
use pallas::network::facades::PeerClient;
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;
use crate::UpstreamCacheRecord;

const DEFAULT_HEADER_TOPIC: (&str,&str) = ("header-topic", "cardano.block.header");
const DEFAULT_BODY_TOPIC: (&str,&str) = ("body-topic", "cardano.block.body");
const DEFAULT_SNAPSHOT_COMPLETION_TOPIC: (&str,&str) = 
    ("snapshot-complietion-topic","cardano.snapshot.complete");

const DEFAULT_NODE_ADDRESS: (&str,&str) = ("node-address", "backbone.cardano.iog.io:3001");
const DEFAULT_MAGIC_NUMBER: (&str,u64) = ("magic-number", 764824073);

const DEFAULT_SYNC_POINT: (&str,SyncPoint) = ("sync-point", SyncPoint::Snapshot);
const DEFAULT_CACHE_DIR: (&str,&str) = ("cache-dir", "upstream-cache");

#[derive(Clone, Debug, serde::Deserialize, PartialEq)]
pub enum SyncPoint {
    #[serde(rename = "origin")]
    Origin,
    #[serde(rename = "tip")]
    Tip,
    #[serde(rename = "cache")]
    Cache,
    #[serde(rename = "snapshot")]
    Snapshot
}

pub struct FetcherConfig {
    pub context: Arc<Context<Message>>,
    pub header_topic: String,
    pub body_topic: String,
    pub sync_point: SyncPoint,
    pub snapshot_completion_topic: String,
    pub node_address: String,
    pub magic_number: u64,
    pub cache_dir: String
}

impl FetcherConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!("Parameter value '{}' for {}", actual, keydef.0);
        actual
    }

    fn conf_enum<'a, T: Deserialize<'a> + std::fmt::Debug>(config: &Arc<Config>, keydef: (&str, T)) -> Result<T> {
        let actual = if config.get_string(keydef.0).is_ok() {
            config
                .get::<T>(keydef.0)
                .or_else(|e| Err(anyhow!("cannot parse {} value: {e}", keydef.0)))?
        } else {
            keydef.1
        };
        info!("Parameter value '{actual:?}' for {}", keydef.0);
        Ok(actual)
    }

    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            context,
            header_topic: Self::conf(&config, DEFAULT_HEADER_TOPIC),
            body_topic: Self::conf(&config, DEFAULT_BODY_TOPIC),
            snapshot_completion_topic: Self::conf(&config, DEFAULT_SNAPSHOT_COMPLETION_TOPIC),
            sync_point: Self::conf_enum::<SyncPoint>(&config, DEFAULT_SYNC_POINT)?,
            magic_number: config.get::<u64>(DEFAULT_MAGIC_NUMBER.0)
                                .unwrap_or(DEFAULT_MAGIC_NUMBER.1),
            node_address: Self::conf(&config, DEFAULT_NODE_ADDRESS),
            cache_dir: Self::conf(&config, DEFAULT_CACHE_DIR)
        }))
    }
}

pub async fn publish_message(cfg: Arc<FetcherConfig>, record: &UpstreamCacheRecord) -> Result<()> {
    let header_msg = Arc::new(Message::Cardano((
         record.id.clone(), 
         CardanoMessage::BlockHeader((*record.hdr).clone())
    )));

    let body_msg = Arc::new(Message::Cardano((
         record.id.clone(),
         CardanoMessage::BlockBody((*record.body).clone())
    )));

    cfg.context.message_bus.publish(&cfg.header_topic, header_msg).await?;
    cfg.context.message_bus.publish(&cfg.body_topic, body_msg).await
}

pub async fn peer_connect(cfg: Arc<FetcherConfig>, role: &str) -> Result<PeerClient> {
    info!("Connecting {role} to {} ({}) ...", cfg.node_address, cfg.magic_number);

    match PeerClient::connect(cfg.node_address.clone(), cfg.magic_number).await {
        Ok(peer) => {
            info!("Connected");
            Ok(peer)
        }
        Err(e) => bail!(
            "Cannot connect {role} to {} ({}): {e}", cfg.node_address, cfg.magic_number
        )
    }
}
