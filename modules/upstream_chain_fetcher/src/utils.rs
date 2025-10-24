use crate::UpstreamCacheRecord;
use acropolis_common::genesis_values::{GenesisDelegs, GenesisValues};
use acropolis_common::messages::{CardanoMessage, Message};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::Context;
use config::Config;
use pallas::network::facades;
use pallas::network::facades::PeerClient;
use serde::Deserialize;
use std::sync::Arc;
use tracing::{error, info};

const DEFAULT_BLOCK_TOPIC: (&str, &str) = ("block-topic", "cardano.block.available");
const DEFAULT_SNAPSHOT_COMPLETION_TOPIC: (&str, &str) =
    ("snapshot-completion-topic", "cardano.snapshot.complete");
const DEFAULT_GENESIS_COMPLETION_TOPIC: (&str, &str) =
    ("genesis-completion-topic", "cardano.sequence.bootstrapped");

const DEFAULT_NODE_ADDRESS: (&str, &str) = ("node-address", "backbone.cardano.iog.io:3001");
const DEFAULT_MAGIC_NUMBER: (&str, u64) = ("magic-number", 764824073);

const DEFAULT_SYNC_POINT: (&str, SyncPoint) = ("sync-point", SyncPoint::Snapshot);
const DEFAULT_CACHE_DIR: (&str, &str) = ("cache-dir", "upstream-cache");

const BYRON_TIMESTAMP: &str = "byron-timestamp";
const SHELLEY_EPOCH: &str = "shelley-epoch";
const SHELLEY_EPOCH_LEN: &str = "shelley-epoch-len";
const SHELLEY_GENESIS_HASH: &str = "shelley-genesis-hash";

#[derive(Clone, Debug, serde::Deserialize, PartialEq)]
pub enum SyncPoint {
    #[serde(rename = "origin")]
    Origin,
    #[serde(rename = "tip")]
    Tip,
    #[serde(rename = "cache")]
    Cache,
    #[serde(rename = "snapshot")]
    Snapshot,
}

pub struct FetcherConfig {
    pub context: Arc<Context<Message>>,
    pub block_topic: String,
    pub sync_point: SyncPoint,
    pub snapshot_completion_topic: String,
    pub genesis_completion_topic: String,
    pub node_address: String,
    pub magic_number: u64,
    pub cache_dir: String,

    pub genesis_values: Option<GenesisValues>,
}

/// Custom option type --- for the purpose of code clarity.
/// Represents outcome of network operation.
pub enum FetchResult<T> {
    Success(T),
    NetworkError,
}

impl FetcherConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!("Parameter value '{}' for {}", actual, keydef.0);
        actual
    }

    fn conf_enum<'a, T: Deserialize<'a> + std::fmt::Debug>(
        config: &Arc<Config>,
        keydef: (&str, T),
    ) -> Result<T> {
        let actual = if config.get_string(keydef.0).is_ok() {
            config
                .get::<T>(keydef.0)
                .map_err(|e| anyhow!("cannot parse {} value: {e}", keydef.0))?
        } else {
            keydef.1
        };
        info!("Parameter value '{actual:?}' for {}", keydef.0);
        Ok(actual)
    }

    fn conf_genesis(config: &Arc<Config>) -> Option<GenesisValues> {
        let byron_timestamp = config.get(BYRON_TIMESTAMP).ok()?;
        let shelley_epoch = config.get(SHELLEY_EPOCH).ok()?;
        let shelley_epoch_len = config.get(SHELLEY_EPOCH_LEN).ok()?;
        let shelley_genesis_hash =
            config.get::<String>(SHELLEY_GENESIS_HASH).ok()?.as_bytes().try_into().unwrap();
        Some(GenesisValues {
            byron_timestamp,
            shelley_epoch,
            shelley_epoch_len,
            shelley_genesis_hash,
            // TODO: load genesis keys from config
            genesis_delegs: GenesisDelegs::from(vec![]),
        })
    }

    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Result<Self> {
        Ok(Self {
            context,
            block_topic: Self::conf(&config, DEFAULT_BLOCK_TOPIC),
            snapshot_completion_topic: Self::conf(&config, DEFAULT_SNAPSHOT_COMPLETION_TOPIC),
            genesis_completion_topic: Self::conf(&config, DEFAULT_GENESIS_COMPLETION_TOPIC),
            sync_point: Self::conf_enum::<SyncPoint>(&config, DEFAULT_SYNC_POINT)?,
            magic_number: config
                .get::<u64>(DEFAULT_MAGIC_NUMBER.0)
                .unwrap_or(DEFAULT_MAGIC_NUMBER.1),
            node_address: Self::conf(&config, DEFAULT_NODE_ADDRESS),
            cache_dir: Self::conf(&config, DEFAULT_CACHE_DIR),
            genesis_values: Self::conf_genesis(&config),
        })
    }

    pub fn slot_to_epoch(&self, slot: u64) -> (u64, u64) {
        self.genesis_values.as_ref().unwrap().slot_to_epoch(slot)
    }

    pub fn slot_to_timestamp(&self, slot: u64) -> u64 {
        self.genesis_values.as_ref().unwrap().slot_to_timestamp(slot)
    }
}

pub async fn publish_message(cfg: Arc<FetcherConfig>, record: &UpstreamCacheRecord) -> Result<()> {
    let message = Arc::new(Message::Cardano((
        record.id.clone(),
        CardanoMessage::BlockAvailable((*record.message).clone()),
    )));

    cfg.context.message_bus.publish(&cfg.block_topic, message).await
}

pub async fn peer_connect(cfg: Arc<FetcherConfig>, role: &str) -> Result<FetchResult<PeerClient>> {
    info!(
        "Connecting {role} to {} ({}) ...",
        cfg.node_address, cfg.magic_number
    );

    match PeerClient::connect(cfg.node_address.clone(), cfg.magic_number).await {
        Ok(peer) => {
            info!("Connected");
            Ok(FetchResult::Success(peer))
        }

        Err(facades::Error::ConnectFailure(e)) => {
            error!("Network error: {e}");
            Ok(FetchResult::NetworkError)
        }

        Err(e) => bail!(
            "Cannot connect {role} to {} ({}): {e}",
            cfg.node_address,
            cfg.magic_number
        ),
    }
}
