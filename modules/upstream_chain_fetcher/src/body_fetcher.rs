//! Acropolis Miniprotocols module for Caryatid
//! Multi-connection, block body fetching part of the client (in separate thread).

use acropolis_common::{
    calculations::slot_to_epoch,
    messages::{BlockBodyMessage, BlockHeaderMessage},
    BlockInfo, BlockStatus, Era,
};
use anyhow::{bail, Result};
use crossbeam::channel::{Receiver, TryRecvError};
use pallas::{
    ledger::traverse::MultiEraHeader,
    network::{
        facades::PeerClient,
        miniprotocols::{chainsync::HeaderContent, Point},
    },
};
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use tracing::{debug, info};

use crate::upstream_cache::{UpstreamCache, UpstreamCacheRecord};
use crate::{utils, utils::FetcherConfig};

pub struct BodyFetcher {
    cfg: Arc<FetcherConfig>,
    peer: PeerClient,
    cache: Option<UpstreamCache>,

    last_epoch: Option<u64>,
}

impl BodyFetcher {
    async fn new(
        cfg: Arc<FetcherConfig>,
        cache: Option<UpstreamCache>,
        last_epoch: Option<u64>,
    ) -> Result<Self> {
        Ok(BodyFetcher {
            cfg: cfg.clone(),
            peer: utils::peer_connect(cfg.clone(), "body fetcher").await?,
            cache,
            last_epoch,
        })
    }

    async fn fetch_block(
        &mut self,
        point: Point,
        block_info: &BlockInfo,
    ) -> Result<Arc<BlockBodyMessage>> {
        // Fetch the block body
        debug!("Requesting single block {point:?}");
        let body = self.peer.blockfetch().fetch_single(point.clone()).await;

        match body {
            Ok(body) => {
                if block_info.number % 100 == 0 {
                    info!(
                        number = block_info.number,
                        size = body.len(),
                        "Fetched block"
                    );
                }

                // Construct message
                Ok(Arc::new(BlockBodyMessage { raw: body }))
            }

            Err(e) => bail!("Can't fetch block at {point:?}: {e}"),
        }
    }

    async fn process_message(&mut self, rolled_back: bool, h: HeaderContent) -> Result<()> {
        // Get Byron sub-tag if any
        let tag = match h.byron_prefix {
            Some((tag, _)) => Some(tag),
            _ => None,
        };

        // Decode header
        let header = MultiEraHeader::decode(h.variant, tag, &h.cbor)?;
        let slot = header.slot();
        let number = header.number();
        let hash = header.hash().to_vec();

        let epoch = slot_to_epoch(slot);
        let new_epoch = match self.last_epoch {
            Some(last_epoch) => epoch != last_epoch,
            None => true,
        };
        self.last_epoch = Some(epoch);

        if new_epoch {
            info!(epoch, number, slot, "New epoch");
        }

        // Derive era from header - not complete but enough to drive
        // MultiEraHeader::decode() again at the receiver
        // TODO do this properly once we understand the values of the 'variant'
        // byte
        let era = match header {
            MultiEraHeader::EpochBoundary(_) => return Ok(()), // Ignore EBBs
            MultiEraHeader::Byron(_) => Era::Byron,
            MultiEraHeader::ShelleyCompatible(_) => Era::Shelley,
            MultiEraHeader::BabbageCompatible(_) => Era::Babbage,
        };

        // Construct message
        let block_info = BlockInfo {
            status: if rolled_back {
                BlockStatus::RolledBack
            } else {
                BlockStatus::Volatile
            }, // TODO vary with 'k'
            slot,
            number,
            hash: hash.clone(),
            epoch,
            new_epoch,
            era,
        };

        let msg_hdr = Arc::new(BlockHeaderMessage { raw: h.cbor });

        // Fetch and publish the block itself - note we need to
        // reconstruct a Point from the header because the one we get
        // in the RollForward is the *tip*, not the next read point
        let fetch_point = Point::Specific(slot, hash);
        let msg_body = self.fetch_block(fetch_point, &block_info).await?;

        let record = UpstreamCacheRecord {
            id: block_info.clone(),
            hdr: msg_hdr.clone(),
            body: msg_body.clone(),
        };

        self.cache.as_mut().map(|c| c.write_record(&record)).transpose()?;
        utils::publish_message(self.cfg.clone(), &record).await?;

        if record.id.number % 100 == 0 {
            info!("Publishing message {}", record.id.number);
        }

        Ok(())
    }

    pub async fn run(
        cfg: Arc<FetcherConfig>,
        cache: Option<UpstreamCache>,
        last_epoch: Option<u64>,
        receiver: Receiver<(bool, HeaderContent)>,
    ) -> Result<()> {
        let mut fetcher = Self::new(cfg, cache, last_epoch).await?;
        loop {
            match receiver.try_recv() {
                Ok((rolled_back, header)) => fetcher.process_message(rolled_back, header).await?,
                Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => sleep(Duration::from_millis(1)).await,
            }
        }
    }
}
