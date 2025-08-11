//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, multi-protocol client interface to the Cardano node

use acropolis_common::{
    calculations::slot_to_epoch,
    messages::{CardanoMessage, Message},
    BlockInfo,
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use crossbeam::channel::{bounded, TrySendError};
use pallas::{
    ledger::traverse::MultiEraHeader,
    network::{
        facades::PeerClient,
        miniprotocols::{
            chainsync::{NextResponse, Tip},
            Point,
        },
    },
};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::sleep};
use tracing::{debug, error, info};

mod body_fetcher;
mod upstream_cache;
mod utils;

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
    /// ChainSync client loop - fetch headers and pass it to body fetching thread
    async fn sync_to_point(
        cfg: Arc<FetcherConfig>,
        peer: Arc<Mutex<PeerClient>>,
        cache: Option<UpstreamCache>,
        start: Point,
    ) -> Result<()> {
        // Find intersect to given point
        let slot = start.slot_or_default();
        info!("Synchronising to slot {slot}");
        let mut my_peer = peer.lock().await;
        let (start, _) = my_peer.chainsync().find_intersect(vec![start]).await?;
        let start = start.ok_or(anyhow!("Intersection for slot {slot} not found"))?;

        // Loop fetching messages
        let mut rolled_back = false;
        let mut response_count = 0;

        let last_epoch: Option<u64> = match slot {
            0 => None,                      // If we're starting from origin
            _ => Some(slot_to_epoch(slot)), // From slot of last block
        };

        let (sender, receiver) = bounded(MAX_BODY_FETCHER_CHANNEL_LENGTH);
        //let cfg_clone = cfg.clone();

        tokio::spawn(async move {
            info!("Starting BodyFetcher...");
            if let Err(e) = BodyFetcher::run(cfg, cache, last_epoch, receiver).await {
                error!("Error in BodyFetcher: {e}");
            }
        });

        loop {
            response_count += 1;
            let next = my_peer.chainsync().request_or_await_next().await?;

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
                            Err(e) => bail!("Cannot send message to BodyFetcher: {e}"),
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
        let peer = Arc::new(Mutex::new(
            utils::peer_connect(cfg.clone(), "header fetcher").await?,
        ));

        match cfg.sync_point {
            SyncPoint::Tip => {
                // Ask for origin but get the tip as well
                let mut my_peer = peer.lock().await;
                let (_, Tip(point, _)) =
                    my_peer.chainsync().find_intersect(vec![Point::Origin]).await?;
                Self::sync_to_point(cfg, peer.clone(), None, point).await?;
            }
            SyncPoint::Origin => {
                Self::sync_to_point(cfg, peer.clone(), None, Point::Origin).await?;
            }
            SyncPoint::Cache => {
                let mut upstream_cache = UpstreamCache::new(&cfg.cache_dir);
                let point = match Self::read_cache(cfg.clone(), &mut upstream_cache).await? {
                    None => Point::Origin,
                    Some(blk) => Point::Specific(blk.slot, blk.hash),
                };

                Self::sync_to_point(cfg, peer.clone(), Some(upstream_cache), point).await?;
            }
            SyncPoint::Snapshot => {
                info!(
                    "Waiting for snapshot completion on {}",
                    cfg.snapshot_completion_topic
                );
                let mut completion_subscription =
                    snapshot_complete.as_mut().ok_or_else(|| anyhow!("Snapshot topic missing"))?;

                match Self::wait_snapshot_completion(&mut completion_subscription).await? {
                    Some(block) => {
                        info!(
                            "Notified snapshot complete at slot {} block number {}",
                            block.slot, block.number
                        );
                        let point = Point::Specific(block.slot, block.hash.clone());
                        Self::sync_to_point(cfg, peer, None, point).await?;
                    }
                    None => info!("Completion not received. Exiting ..."),
                }
            }
        }
        Ok(())
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = FetcherConfig::new(context.clone(), config)?;
        let mut subscription = match cfg.sync_point {
            SyncPoint::Snapshot => {
                Some(cfg.context.subscribe(&cfg.snapshot_completion_topic).await?)
            }
            _ => None,
        };

        context.clone().run(async move {
            Self::run_chain_sync(cfg, &mut subscription)
                .await
                .unwrap_or_else(|e| error!("Chain sync failed: {e}"));
        });

        Ok(())
    }
}
