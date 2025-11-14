//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, block body fetching part of the client (in separate thread).

use acropolis_common::{
    messages::RawBlockMessage,
    upstream_cache::{UpstreamCache, UpstreamCacheRecord},
    BlockHash, BlockInfo, BlockStatus, Era,
};
use anyhow::{bail, Result};
use crossbeam::channel::{Receiver, TryRecvError};
use pallas::{
    ledger::traverse::MultiEraHeader,
    network::{
        facades::PeerClient,
        miniprotocols::{blockfetch, chainsync::HeaderContent, Point},
    },
};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::sleep};
use tracing::{debug, error, info};

use crate::{
    utils,
    utils::{
        FetchResult,
        FetchResult::{NetworkError, Success},
        FetcherConfig,
    },
};

pub struct BodyFetcher {
    cfg: Arc<FetcherConfig>,
    peer: PeerClient,
    cache: Option<Arc<Mutex<UpstreamCache>>>,

    prev_epoch: Option<u64>,
}

impl BodyFetcher {
    async fn new(
        cfg: Arc<FetcherConfig>,
        cache: Option<Arc<Mutex<UpstreamCache>>>,
        prev_epoch: Option<u64>,
    ) -> Result<FetchResult<Self>> {
        let peer_opt = utils::peer_connect(cfg.clone(), "body fetcher").await?;

        match peer_opt {
            NetworkError => Ok(NetworkError),
            Success(peer) => Ok(Success(BodyFetcher {
                cfg: cfg.clone(),
                peer,
                cache,
                prev_epoch,
            })),
        }
    }

    async fn fetch_block(&mut self, point: Point) -> Result<FetchResult<Arc<Vec<u8>>>> {
        // Fetch the block body
        debug!("Requesting single block {point:?}");
        let body = self.peer.blockfetch().fetch_single(point.clone()).await;

        match body {
            Ok(body) => Ok(Success(Arc::new(body))),
            Err(blockfetch::ClientError::Plexer(e)) => {
                error!("Can't fetch block at {point:?}: {e}, will try to restart");
                Ok(NetworkError)
            }
            Err(e) => bail!("Irrecoverable error in blockfetch.fetch_single at {point:?}: {e}",),
        }
    }

    fn make_era(header: &MultiEraHeader, variant: u8) -> Result<Option<Era>> {
        // It seems that `variant` field is 'TipInfo' from Haskell Node:
        // ouroboros-consensus-cardano/Ouroboros/Consensus/Cardano/Block.hs
        // TODO: should we parse protocol version from header?
        match header {
            MultiEraHeader::EpochBoundary(_) => Ok(None), // Ignore EBBs
            MultiEraHeader::Byron(_) => Ok(Some(Era::Byron)),
            MultiEraHeader::ShelleyCompatible(_) => match variant {
                // TPraos eras
                1 => Ok(Some(Era::Shelley)),
                2 => Ok(Some(Era::Allegra)),
                3 => Ok(Some(Era::Mary)),
                4 => Ok(Some(Era::Alonzo)),
                x => bail!("Impossible header variant {x} for ShelleyCompatible (TPraos)"),
            },
            MultiEraHeader::BabbageCompatible(_) => match variant {
                // Praos eras
                5 => Ok(Some(Era::Babbage)),
                6 => Ok(Some(Era::Conway)),
                x => bail!("Impossible header variant {x} for BabbaageCompatible (Praos)"),
            },
        }
    }

    fn make_block_info(
        &self,
        rolled_back: bool,
        last_epoch: Option<u64>,
        era: Era,
        header: &MultiEraHeader,
    ) -> Result<BlockInfo> {
        let slot = header.slot();
        let number = header.number();
        let hash = *header.hash();

        let (epoch, epoch_slot) = self.cfg.slot_to_epoch(slot);
        let new_epoch = match last_epoch {
            Some(last_epoch) => epoch != last_epoch,
            None => true,
        };
        let timestamp = self.cfg.slot_to_timestamp(slot);

        Ok(BlockInfo {
            status: if rolled_back {
                BlockStatus::RolledBack
            } else {
                BlockStatus::Volatile
            }, // TODO vary with 'k'
            slot,
            number,
            hash: BlockHash::from(hash),
            epoch,
            epoch_slot,
            new_epoch,
            timestamp,
            era,
        })
    }

    /// Returns Ok(None) if block could not be retrieved due to network problems
    async fn fetch_and_construct(
        &mut self,
        block_info: &BlockInfo,
        h: HeaderContent,
    ) -> Result<FetchResult<UpstreamCacheRecord>> {
        // Fetch the block itself - note we need to
        // reconstruct a Point from the header because the one we get
        // in the RollForward is the *tip*, not the next read point
        let fetch_point = Point::Specific(block_info.slot, block_info.hash.to_vec());
        let raw_body = match self.fetch_block(fetch_point).await? {
            Success(body) => body,
            NetworkError => return Ok(NetworkError),
        };

        let message = Arc::new(RawBlockMessage {
            header: h.cbor,
            body: raw_body.to_vec(),
        });
        let record = UpstreamCacheRecord {
            id: block_info.clone(),
            message: message.clone(),
        };

        Ok(Success(record))
    }

    // Returns block info of the message, if it was successfully published and cached.
    async fn fetch_and_publish(
        &mut self,
        rolled_back: bool,
        h: HeaderContent,
    ) -> Result<FetchResult<Option<BlockInfo>>> {
        // Get Byron sub-tag if any
        let hdr_tag = match h.byron_prefix {
            Some((tag, _)) => Some(tag),
            _ => None,
        };
        let hdr_variant = h.variant;

        // Decode header
        let header = MultiEraHeader::decode(hdr_variant, hdr_tag, &h.cbor)?;
        let era = match Self::make_era(&header, hdr_variant)? {
            Some(era) => era,
            None => return Ok(Success(None)),
        };

        // Build block info
        let blk = self.make_block_info(rolled_back, self.prev_epoch, era, &header)?;
        self.prev_epoch = Some(blk.epoch);

        // Fetch block body and construct record for caching/publishing
        match self.fetch_and_construct(&blk, h).await? {
            Success(record) => {
                if blk.new_epoch {
                    info!(
                        blk.epoch,
                        blk.number, blk.slot, hdr_variant, hdr_tag, "New epoch"
                    );
                }

                if record.id.number % 100 == 0 {
                    info!("Publishing message {}", record.id.number);
                }

                // Publish block body and write it to cache (if it is enabled)
                if let Some(cache_mutex) = &self.cache {
                    let mut cache = cache_mutex.lock().await;
                    cache.write_record(&record)?;
                }
                utils::publish_message(self.cfg.clone(), &record).await?;

                Ok(Success(Some(blk)))
            }
            NetworkError => Ok(NetworkError),
        }
    }

    pub async fn run(
        cfg: Arc<FetcherConfig>,
        cache: Option<Arc<Mutex<UpstreamCache>>>,
        last_epoch: Option<u64>,
        receiver: Receiver<(bool, HeaderContent)>,
    ) -> Result<Option<BlockInfo>> {
        let fetcher_opt = Self::new(cfg, cache, last_epoch).await?;
        let mut fetcher = match fetcher_opt {
            Success(f) => f,
            NetworkError => return Ok(None),
        };

        let mut last_successful_block = None;
        loop {
            match receiver.try_recv() {
                Ok((rolled_back, header)) => {
                    match fetcher.fetch_and_publish(rolled_back, header).await? {
                        Success(b @ Some(_)) => last_successful_block = b,
                        Success(None) => (),
                        NetworkError => break,
                    }
                }
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => sleep(Duration::from_millis(1)).await,
            }
        }
        Ok(last_successful_block)
    }
}
