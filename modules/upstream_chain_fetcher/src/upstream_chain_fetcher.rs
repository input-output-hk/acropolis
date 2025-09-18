//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, multi-protocol client interface to the Cardano node

use acropolis_common::{
    genesis_values::GenesisValues,
    messages::{CardanoMessage, Message},
    BlockInfo,
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use crossbeam::channel::{bounded, Sender, TrySendError};
use pallas::network::facades::PeerClient;
use pallas::network::miniprotocols::chainsync::{ClientError, HeaderContent};
use pallas::{
    ledger::traverse::MultiEraHeader,
    network::miniprotocols::{
        chainsync::{NextResponse, Tip},
        Point,
    },
};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::sleep};
use tracing::{debug, error, info};

mod body_fetcher;
mod upstream_cache;
mod utils;

use crate::utils::FetchResult;
use body_fetcher::BodyFetcher;
use upstream_cache::{UpstreamCache, UpstreamCacheRecord};
use utils::{FetcherConfig, SyncPoint};

const MAX_BODY_FETCHER_CHANNEL_LENGTH: usize = 100;

/// Upstream chain fetcher module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "upstream-chain-fetcher",
    description = "Mini-protocol chain fetcher from an upstream Cardano node"
)]
pub struct UpstreamChainFetcher;

impl UpstreamChainFetcher {
    async fn sync_to_point_loop(
        sender: Sender<(bool, HeaderContent)>,
        start: Point,
        my_peer: &mut PeerClient,
    ) -> Result<()> {
        // Loop fetching messages
        let mut rolled_back = false;
        let mut response_count = 0;

        loop {
            response_count += 1;
            let next = match my_peer.chainsync().request_or_await_next().await {
                Err(ClientError::Plexer(e)) => {
                    error!("Connection error for chainsync: {e}, will try to restart");
                    return Ok(());
                }
                Err(e) => bail!("Connection error for chainsync: {e}, exiting"),
                Ok(next) => next,
            };

            match next {
                NextResponse::RollForward(h, Tip(tip_point, _)) => {
                    debug!("RollForward, tip is {tip_point:?}");

                    let tag = match h.byron_prefix {
                        Some((tag, _)) => Some(tag),
                        _ => None,
                    };

                    if response_count % 100 == 0 {
                        let header = MultiEraHeader::decode(h.variant, tag, &h.cbor)?;
                        let number = header.number();
                        info!("Fetching header {}", number);
                    }

                    let mut for_send = (rolled_back, h);

                    'sender: loop {
                        for_send = match sender.try_send(for_send) {
                            Ok(()) => break 'sender,
                            Err(TrySendError::Full(fs)) => fs,
                            Err(TrySendError::Disconnected(_)) => {
                                error!("BodyFetcher disconnected, will try to restart");
                                return Ok(());
                            }
                        };
                        sleep(Duration::from_millis(100)).await;
                    }

                    rolled_back = false;
                }

                // TODO The first message after sync start always comes with 'RollBackward'.
                // Here we suppress this status (since it says nothing about actual rollbacks,
                // but about our sync restart). Can there arise any problems?
                NextResponse::RollBackward(point, _) if start == point && response_count == 1 => (),

                // TODO Handle RollBackward, publish sync message
                NextResponse::RollBackward(point, _) => {
                    info!("RollBackward to {point:?}");
                    rolled_back = true;
                }

                _ => debug!("Ignoring message: {next:?}"),
            }
        }
    }

    /// ChainSync client loop - fetch headers and pass it to body fetching thread
    /// Returns last read block, if there is a reason to restart the loop.
    /// If the loop did not read any block, returns None.
    async fn sync_to_point_impl(
        cfg: Arc<FetcherConfig>,
        cache: Option<Arc<Mutex<UpstreamCache>>>,
        start: Point,
    ) -> Result<Option<BlockInfo>> {
        // Find intersect to given point
        let slot = start.slot_or_default();
        info!("Synchronising to slot {slot}");

        let peer = utils::peer_connect(cfg.clone(), "header fetcher").await?;
        let mut my_peer = match peer {
            FetchResult::NetworkError => return Ok(None),
            FetchResult::Success(p) => p,
        };

        // TODO: check for lost connection in find_intersect; skipped now to keep code simpler
        let (start, _) = my_peer.chainsync().find_intersect(vec![start]).await?;
        let start = start.ok_or(anyhow!("Intersection for slot {slot} not found"))?;

        let last_epoch: Option<u64> = match slot {
            0 => None,                            // If we're starting from origin
            _ => Some(cfg.slot_to_epoch(slot).0), // From slot of last block
        };

        let (sender, receiver) = bounded(MAX_BODY_FETCHER_CHANNEL_LENGTH);

        let body_fetcher_handle = tokio::spawn(async move {
            info!("Starting BodyFetcher...");
            BodyFetcher::run(cfg, cache, last_epoch, receiver).await
        });

        Self::sync_to_point_loop(sender, start, &mut my_peer).await?;

        let outcome = body_fetcher_handle.await??;
        Ok(outcome)
    }

    async fn sync_to_point(
        cfg: Arc<FetcherConfig>,
        cache: Option<Arc<Mutex<UpstreamCache>>>,
        mut start: Point,
    ) -> Result<()> {
        loop {
            let stops_at =
                Self::sync_to_point_impl(cfg.clone(), cache.clone(), start.clone()).await?;

            if let Some(blk) = stops_at {
                start = Point::new(blk.slot, blk.hash.to_vec());
            }
        }
    }

    async fn read_cache(
        cfg: Arc<FetcherConfig>,
        cache: &mut UpstreamCache,
    ) -> Result<Option<BlockInfo>> {
        let mut last_block = None;
        cache.start_reading()?;

        while let Some(record) = cache.read_record()? {
            last_block = Some(record.id.clone());
            utils::publish_message(cfg.clone(), &record).await?;
            cache.next_record()?;
        }

        Ok(last_block)
    }

    async fn wait_genesis_completion(
        subscription: &mut Box<dyn Subscription<Message>>,
    ) -> Result<GenesisValues> {
        let (_, message) = subscription.read().await?;
        match message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                Ok(complete.values.clone())
            }
            msg => bail!("Unexpected message in genesis completion topic: {msg:?}"),
        }
    }

    async fn wait_snapshot_completion(
        subscription: &mut Box<dyn Subscription<Message>>,
    ) -> Result<Option<BlockInfo>> {
        let Ok((_, message)) = subscription.read().await else {
            return Ok(None);
        };

        match message.as_ref() {
            Message::Cardano((blk, CardanoMessage::SnapshotComplete)) => Ok(Some(blk.clone())),
            msg => bail!("Unexpected message in completion topic: {msg:?}"),
        }
    }

    async fn run_chain_sync(
        cfg: Arc<FetcherConfig>,
        snapshot_complete: &mut Option<Box<dyn Subscription<Message>>>,
    ) -> Result<()> {
        match cfg.sync_point {
            SyncPoint::Tip => {
                // Ask for origin but get the tip as well
                let mut peer = match utils::peer_connect(cfg.clone(), "tip fetcher").await? {
                    FetchResult::NetworkError => bail!("Cannot get tip: network error"),
                    FetchResult::Success(p) => p,
                };

                let (_, Tip(point, _)) =
                    peer.chainsync().find_intersect(vec![Point::Origin]).await?;
                Self::sync_to_point(cfg, None, point).await?;
            }
            SyncPoint::Origin => {
                Self::sync_to_point(cfg, None, Point::Origin).await?;
            }
            SyncPoint::Cache => {
                let mut upstream_cache = UpstreamCache::new(&cfg.cache_dir);
                let point = match Self::read_cache(cfg.clone(), &mut upstream_cache).await? {
                    None => Point::Origin,
                    Some(blk) => Point::Specific(blk.slot, blk.hash.to_vec()),
                };

                let upstream_cache_mutex = Arc::new(Mutex::new(upstream_cache));
                Self::sync_to_point(cfg, Some(upstream_cache_mutex), point).await?;
            }
            SyncPoint::Snapshot => {
                info!(
                    "Waiting for snapshot completion on {}",
                    cfg.snapshot_completion_topic
                );
                let mut completion_subscription = snapshot_complete
                    .as_mut()
                    .ok_or_else(|| anyhow!("Snapshot topic subscription missing"))?;

                match Self::wait_snapshot_completion(&mut completion_subscription).await? {
                    Some(block) => {
                        info!(
                            "Notified snapshot complete at slot {} block number {}",
                            block.slot, block.number
                        );
                        let point = Point::Specific(block.slot, block.hash.to_vec());
                        Self::sync_to_point(cfg, None, point).await?;
                    }
                    None => info!("Completion not received. Exiting ..."),
                }
            }
        }
        Ok(())
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let mut cfg = FetcherConfig::new(context.clone(), config)?;
        let genesis_complete = if cfg.genesis_values.is_none() {
            Some(cfg.context.subscribe(&cfg.genesis_completion_topic).await?)
        } else {
            None
        };
        let mut snapshot_complete = match cfg.sync_point {
            SyncPoint::Snapshot => {
                Some(cfg.context.subscribe(&cfg.snapshot_completion_topic).await?)
            }
            _ => None,
        };

        context.clone().run(async move {
            if let Some(mut genesis_complete) = genesis_complete {
                let genesis = Self::wait_genesis_completion(&mut genesis_complete)
                    .await
                    .unwrap_or_else(|err| panic!("could not fetch genesis: {err}"));
                cfg.genesis_values = Some(genesis);
            }
            let cfg = Arc::new(cfg);
            Self::run_chain_sync(cfg, &mut snapshot_complete)
                .await
                .unwrap_or_else(|e| error!("Chain sync failed: {e}"));
        });

        Ok(())
    }
}
