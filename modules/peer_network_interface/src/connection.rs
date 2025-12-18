use std::time::Duration;

use acropolis_common::{BlockHash, Era};
use anyhow::{Result, bail};
pub use pallas::network::miniprotocols::Point;
use pallas::{
    ledger::traverse::MultiEraHeader,
    network::{
        facades::PeerClient,
        miniprotocols::{blockfetch, chainsync},
    },
};
use tokio::{
    select,
    sync::{mpsc, oneshot},
};
use tracing::error;

use crate::network::PeerMessageSender;

pub struct PeerConnection {
    pub address: String,
    chainsync: mpsc::UnboundedSender<ChainsyncCommand>,
    blockfetch: mpsc::UnboundedSender<BlockfetchCommand>,
}

impl PeerConnection {
    pub fn new(address: String, magic: u32, sender: PeerMessageSender, delay: Duration) -> Self {
        let worker = PeerConnectionWorker {
            address: address.clone(),
            magic,
            sender,
        };
        let (chainsync_tx, chainsync_rx) = mpsc::unbounded_channel();
        let (blockfetch_tx, blockfetch_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            worker.run(chainsync_rx, blockfetch_rx).await;
        });
        Self {
            address,
            chainsync: chainsync_tx,
            blockfetch: blockfetch_tx,
        }
    }

    pub async fn find_tip(&self) -> Result<Point> {
        let (tx, rx) = oneshot::channel();
        self.chainsync.send(ChainsyncCommand::FindTip(tx))?;
        Ok(rx.await?)
    }

    pub fn find_intersect(&self, points: Vec<Point>) -> Result<()> {
        self.chainsync.send(ChainsyncCommand::FindIntersect(points))?;
        Ok(())
    }

    pub fn request_block(&self, hash: BlockHash, slot: u64) -> Result<()> {
        self.blockfetch.send(BlockfetchCommand::Fetch(hash, slot))?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum PeerEvent {
    ChainSync(PeerChainSyncEvent),
    BlockFetched(BlockFetched),
    Disconnected,
}

#[derive(Debug)]
pub enum PeerChainSyncEvent {
    RollForward(Header, Point),
    RollBackward(Point, Point),
    IntersectNotFound(Point),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub hash: BlockHash,
    pub slot: u64,
    pub number: u64,
    pub bytes: Vec<u8>,
    pub era: Era,
}

#[derive(Debug)]
pub struct BlockFetched {
    pub slot: u64,
    pub hash: BlockHash,
    pub body: Vec<u8>,
}

struct PeerConnectionWorker {
    address: String,
    magic: u32,
    sender: PeerMessageSender,
}

impl PeerConnectionWorker {
    async fn run(
        mut self,
        chainsync: mpsc::UnboundedReceiver<ChainsyncCommand>,
        blockfetch: mpsc::UnboundedReceiver<BlockfetchCommand>,
    ) {
        if let Err(err) = self.do_run(chainsync, blockfetch).await {
            error!(peer = self.address, "{err:#}");
        }
        let _ = self.sender.write(PeerEvent::Disconnected).await;
    }

    async fn do_run(
        &mut self,
        chainsync: mpsc::UnboundedReceiver<ChainsyncCommand>,
        blockfetch: mpsc::UnboundedReceiver<BlockfetchCommand>,
    ) -> Result<()> {
        let client = PeerClient::connect(self.address.clone(), self.magic.into()).await?;
        select! {
            res = self.run_chainsync(client.chainsync, chainsync) => res,
            res = self.run_blockfetch(client.blockfetch, blockfetch) => res,
        }
    }

    async fn run_chainsync(
        &self,
        mut client: chainsync::N2NClient,
        mut commands: mpsc::UnboundedReceiver<ChainsyncCommand>,
    ) -> Result<()> {
        let mut reached = None;
        loop {
            select! {
                msg = client.request_or_await_next(), if reached.is_some() => {
                    if let Some(parsed) = self.parse_chainsync_message(msg?)? {
                        reached = Some(parsed.point);
                        self.sender.write(PeerEvent::ChainSync(parsed.event)).await?;
                    }
                }
                cmd = commands.recv() => {
                    let Some(cmd) = cmd else {
                        bail!("parent process has disconnected");
                    };
                    if !client.has_agency() {
                        // To run find_intersect, we must have agency.
                        // If we don't, it's because we requested the next response already.
                        // There's no way to cancel that request, so just wait for it to finish.
                        client.recv_while_must_reply().await?;
                    };
                    match cmd {
                        ChainsyncCommand::FindIntersect(points) => {
                            let (point, tip) = client.find_intersect(points).await?;
                            reached = point;
                            if reached.is_none() {
                                self.sender.write(PeerEvent::ChainSync(PeerChainSyncEvent::IntersectNotFound(tip.0))).await?;
                            }
                        }
                        ChainsyncCommand::FindTip(done) => {
                            let points = reached.as_slice().to_vec();
                            let (_, tip) = client.find_intersect(points).await?;
                            if done.send(tip.0).is_err() {
                                bail!("parent process has disconnected");
                            }
                        }
                    }
                }
            }
        }
    }

    async fn run_blockfetch(
        &self,
        mut client: blockfetch::Client,
        mut commands: mpsc::UnboundedReceiver<BlockfetchCommand>,
    ) -> Result<()> {
        while let Some(BlockfetchCommand::Fetch(hash, slot)) = commands.recv().await {
            let point = Point::Specific(slot, hash.to_vec());
            let body = client.fetch_single(point).await?;
            self.sender.write(PeerEvent::BlockFetched(BlockFetched { slot, hash, body })).await?;
        }
        bail!("parent process has disconnected");
    }

    fn parse_chainsync_message(
        &self,
        msg: chainsync::NextResponse<chainsync::HeaderContent>,
    ) -> Result<Option<ParsedChainsyncMessage>> {
        match msg {
            chainsync::NextResponse::RollForward(header, tip) => {
                let Some(parsed) = self.parse_header(header)? else {
                    return Ok(None);
                };
                let point = Point::Specific(parsed.slot, parsed.hash.to_vec());
                Ok(Some(ParsedChainsyncMessage {
                    point,
                    event: PeerChainSyncEvent::RollForward(parsed, tip.0),
                }))
            }
            chainsync::NextResponse::RollBackward(point, tip) => Ok(Some(ParsedChainsyncMessage {
                point: point.clone(),
                event: PeerChainSyncEvent::RollBackward(point, tip.0),
            })),
            chainsync::NextResponse::Await => Ok(None),
        }
    }

    fn parse_header(&self, header: chainsync::HeaderContent) -> Result<Option<Header>> {
        let hdr_tag = header.byron_prefix.map(|p| p.0);
        let hdr_variant = header.variant;
        let hdr = MultiEraHeader::decode(hdr_variant, hdr_tag, &header.cbor)?;
        if hdr.as_eb().is_some() {
            // skip EpochBoundary blocks
            return Ok(None);
        }
        let era = Era::try_from(hdr_variant)?;
        Ok(Some(Header {
            hash: BlockHash::new(*hdr.hash()),
            slot: hdr.slot(),
            number: hdr.number(),
            bytes: header.cbor,
            era,
        }))
    }
}

enum ChainsyncCommand {
    FindIntersect(Vec<Point>),
    FindTip(oneshot::Sender<Point>),
}

struct ParsedChainsyncMessage {
    point: Point,
    event: PeerChainSyncEvent,
}

enum BlockfetchCommand {
    Fetch(BlockHash, u64),
}
