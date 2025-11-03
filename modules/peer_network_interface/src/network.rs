use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

use crate::{
    BlockSink,
    connection::{Header, PeerChainSyncEvent, PeerConnection, PeerEvent},
};
use acropolis_common::BlockHash;
use anyhow::{Context as _, Result, bail};
use pallas::network::miniprotocols::Point;
use tokio::sync::mpsc;
use tracing::{info, warn};

pub struct NetworkManager {
    network_magic: u64,
    next_id: u64,
    peers: HashMap<PeerId, PeerConnection>,
    preferred_upstream: Option<PeerId>,
    blocks_to_fetch: VecDeque<Header>,
    blocks: HashMap<BlockHash, BlockStatus>,
    head: Option<Point>,
    rolled_back: bool,
    events: mpsc::Receiver<NetworkEvent>,
    events_sender: mpsc::Sender<NetworkEvent>,
    block_sink: BlockSink,
    published_blocks: u64,
}

impl NetworkManager {
    pub fn new(
        network_magic: u64,
        events: mpsc::Receiver<NetworkEvent>,
        events_sender: mpsc::Sender<NetworkEvent>,
        block_sink: BlockSink,
    ) -> Self {
        Self {
            network_magic,
            next_id: 0,
            peers: HashMap::new(),
            preferred_upstream: None,
            blocks_to_fetch: VecDeque::new(),
            blocks: HashMap::new(),
            head: None,
            rolled_back: false,
            events,
            events_sender,
            block_sink,
            published_blocks: 0,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        while let Some(event) = self.events.recv().await {
            match event {
                NetworkEvent::NewConnection { address, delay } => {
                    self.handle_new_connection(address, delay).await
                }
                NetworkEvent::PeerUpdate { peer, event } => {
                    self.handle_peer_update(peer, event).await?
                }
            }
        }
        bail!("event sink closed")
    }

    pub async fn handle_new_connection(&mut self, address: String, delay: Duration) {
        let id = PeerId(self.next_id);
        self.next_id += 1;
        let sender = PeerMessageSender {
            sink: self.events_sender.clone(),
            id,
        };
        let conn = PeerConnection::new(address, self.network_magic, sender, delay);
        if self.preferred_upstream.is_none() {
            self.peers.insert(id, conn);
            self.set_preferred_upstream(id).await;
        } else {
            if let Some(head) = self.head.clone()
                && let Err(error) = conn.find_intersect(vec![head]).await
            {
                warn!("could not sync {}: {error}", conn.address);
            }
            self.peers.insert(id, conn);
        }
    }

    pub async fn sync_to_tip(&mut self) -> Result<()> {
        loop {
            let Some(upstream) = self.preferred_upstream else {
                bail!("no peers");
            };
            let Some(conn) = self.peers.get(&upstream) else {
                bail!("preferred upstream not found");
            };
            match conn.find_tip().await {
                Ok(point) => {
                    self.sync_to_point(point).await;
                    return Ok(());
                }
                Err(e) => {
                    warn!("could not fetch tip from {}: {e}", conn.address);
                    self.handle_disconnect(upstream).await?;
                }
            }
        }
    }

    pub async fn sync_to_point(&mut self, point: Point) {
        for conn in self.peers.values() {
            if let Err(error) = conn.find_intersect(vec![point.clone()]).await {
                warn!("could not sync {}: {error}", conn.address);
            }
        }
    }

    async fn handle_peer_update(&mut self, peer: PeerId, event: PeerEvent) -> Result<()> {
        let is_preferred = self.preferred_upstream.is_some_and(|id| id == peer);
        match event {
            PeerEvent::ChainSync(PeerChainSyncEvent::RollForward(header)) => {
                let id = header.hash;
                let status =
                    self.blocks.entry(id).or_insert(BlockStatus::Announced(header, vec![]));
                match status {
                    BlockStatus::Announced(header, peers) => {
                        peers.push(peer);
                        if is_preferred {
                            self.blocks_to_fetch.push_back(header.clone());
                            // Request the block from the first peer which announced it
                            for announcer in peers.clone() {
                                let Some(peer) = self.peers.get(&announcer) else {
                                    continue;
                                };
                                if let Err(e) = peer.request_block(header.hash, header.slot).await
                                {
                                    warn!("could not request block from {}: {e}", peer.address);
                                    self.handle_disconnect(announcer).await?
                                }
                                break; // only fetch from one
                            }
                        }
                    }
                    BlockStatus::Fetched(_) => {
                        if is_preferred {
                            // Chainsync has requested a block which we've already fetched,
                            // so we might be able to publish one or more.
                            self.publish_blocks().await?;
                        }
                    }
                }
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::RollBackward(point)) => {
                if is_preferred {
                    match point {
                        Point::Origin => {
                            self.blocks_to_fetch.clear();
                            self.rolled_back = true;
                        }
                        Point::Specific(slot, _) => {
                            let mut already_sent = true;
                            while let Some(newest) = self.blocks_to_fetch.back() {
                                if newest.slot == slot {
                                    already_sent = false;
                                    break;
                                } else {
                                    self.blocks_to_fetch.pop_back();
                                }
                            }
                            if already_sent {
                                self.rolled_back = true;
                            }
                        }
                    }
                }
            }
            PeerEvent::BlockFetched(fetched) => {
                let Some(block) = self.blocks.get_mut(&fetched.hash) else {
                    return Ok(());
                };
                block.set_body(&fetched.body);
                self.publish_blocks().await?;
            }
            PeerEvent::Disconnected => self.handle_disconnect(peer).await?,
        }
        Ok(())
    }

    async fn handle_disconnect(&mut self, peer: PeerId) -> Result<()> {
        let Some(conn) = self.peers.remove(&peer) else {
            return Ok(());
        };
        let is_preferred = self.preferred_upstream.is_some_and(|id| id == peer);
        if is_preferred && let Some(new_preferred) = self.peers.keys().next().copied() {
            self.set_preferred_upstream(new_preferred).await;
        }
        self.events_sender
            .send(NetworkEvent::NewConnection {
                address: conn.address,
                delay: Duration::from_secs(5),
            })
            .await?;
        Ok(())
    }

    async fn set_preferred_upstream(&mut self, peer: PeerId) {
        self.preferred_upstream = Some(peer);
        if let Some(head) = self.head.clone() {
            self.sync_to_point(head).await;
        }
    }

    async fn publish_blocks(&mut self) -> Result<()> {
        while let Some(header) = self.blocks_to_fetch.front() {
            let Some(BlockStatus::Fetched(body)) = self.blocks.get(&header.hash) else {
                break;
            };
            self.block_sink.announce(header, body, self.rolled_back).await?;
            self.published_blocks += 1;
            if self.published_blocks.is_multiple_of(100) {
                info!("Published block {}", header.number);
            }
            self.head = Some(Point::Specific(header.slot, header.hash.to_vec()));
            self.rolled_back = false;
            self.blocks_to_fetch.pop_front();
        }
        Ok(())
    }
}

pub enum NetworkEvent {
    NewConnection { address: String, delay: Duration },
    PeerUpdate { peer: PeerId, event: PeerEvent },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PeerId(u64);

pub struct PeerMessageSender {
    id: PeerId,
    sink: mpsc::Sender<NetworkEvent>,
}
impl PeerMessageSender {
    pub async fn write(&self, event: PeerEvent) -> Result<()> {
        self.sink
            .send(NetworkEvent::PeerUpdate {
                peer: self.id,
                event,
            })
            .await
            .context("network manager has shut down")
    }
}

enum BlockStatus {
    Announced(Header, Vec<PeerId>),
    Fetched(Vec<u8>),
}
impl BlockStatus {
    fn set_body(&mut self, body: &[u8]) {
        if let Self::Announced(_, _) = self {
            *self = Self::Fetched(body.to_vec());
        }
    }
}
