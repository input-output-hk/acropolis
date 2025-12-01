use std::{collections::BTreeMap, time::Duration};

use crate::{
    BlockSink,
    chain_state::ChainState,
    connection::{PeerChainSyncEvent, PeerConnection, PeerEvent},
};
use acropolis_common::BlockHash;
use anyhow::{Context as _, Result, bail};
use pallas::network::miniprotocols::Point;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

struct PeerData {
    conn: PeerConnection,
    reqs: Vec<(BlockHash, u64)>,
}
impl PeerData {
    fn new(conn: PeerConnection) -> Self {
        Self { conn, reqs: vec![] }
    }

    fn find_intersect(&self, points: Vec<Point>) {
        if let Err(error) = self.conn.find_intersect(points) {
            warn!("could not sync {}: {error:#}", self.conn.address);
        }
    }

    fn request_block(&mut self, hash: BlockHash, slot: u64) -> bool {
        if self.reqs.contains(&(hash, slot)) {
            return true;
        }
        if let Err(error) = self.conn.request_block(hash, slot) {
            warn!(
                "could not request block from {}: {error:#}",
                self.conn.address
            );
            return false;
        }
        self.reqs.push((hash, slot));
        true
    }

    fn ack_block(&mut self, hash: BlockHash) {
        self.reqs.retain(|(h, _)| *h != hash);
    }
}

pub struct NetworkManager {
    network_magic: u64,
    next_id: u64,
    peers: BTreeMap<PeerId, PeerData>,
    chain: ChainState,
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
            peers: BTreeMap::new(),
            chain: ChainState::new(),
            events,
            events_sender,
            block_sink,
            published_blocks: 0,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        while let Some(event) = self.events.recv().await {
            self.on_network_event(event).await?;
        }

        Ok(())
    }

    async fn on_network_event(&mut self, event: NetworkEvent) -> Result<()> {
        match event {
            NetworkEvent::PeerUpdate { peer, event } => {
                self.handle_peer_update(peer, event);
                self.publish_blocks().await?;
            }
            NetworkEvent::SyncPointUpdate { point } => {
                self.chain = ChainState::new();

                for peer in self.peers.values_mut() {
                    peer.reqs.clear();
                }

                if let Point::Specific(slot, _) = point {
                    let (epoch, _) = self.block_sink.genesis_values.slot_to_epoch(slot);
                    self.block_sink.last_epoch = Some(epoch);
                }

                if let Some((&peer_id, _)) = self.peers.iter().next() {
                    self.set_preferred_upstream(peer_id);
                } else {
                    warn!("Sync requested but no upstream peers available");
                }

                self.sync_to_point(point);
            }
        }

        Ok(())
    }

    pub fn handle_new_connection(&mut self, address: String, delay: Duration) {
        let id = PeerId(self.next_id);
        self.next_id += 1;
        let sender = PeerMessageSender {
            sink: self.events_sender.clone(),
            id,
        };
        let conn = PeerConnection::new(address, self.network_magic, sender, delay);
        let peer = PeerData::new(conn);
        let points = self.chain.choose_points_for_find_intersect();
        if !points.is_empty() {
            peer.find_intersect(points);
        }
        self.peers.insert(id, peer);
        if self.chain.preferred_upstream.is_none() {
            self.set_preferred_upstream(id);
        }
    }

    pub async fn sync_to_tip(&mut self) -> Result<()> {
        loop {
            let Some(upstream) = self.chain.preferred_upstream else {
                bail!("no peers");
            };
            let Some(peer) = self.peers.get(&upstream) else {
                bail!("preferred upstream not found");
            };
            match peer.conn.find_tip().await {
                Ok(point) => {
                    self.sync_to_point(point);
                    return Ok(());
                }
                Err(e) => {
                    warn!("could not fetch tip from {}: {e:#}", peer.conn.address);
                    self.handle_disconnect(upstream);
                }
            }
        }
    }

    pub fn sync_to_point(&mut self, point: Point) {
        for peer in self.peers.values() {
            peer.find_intersect(vec![point.clone()]);
        }
    }

    // Implementation note: this method is deliberately synchronous/non-blocking.
    // The task which handles network events should only block when waiting for new messages,
    // or when publishing messages to other modules. This avoids deadlock; if our event queue
    // is full and this method is blocked on writing to it, the queue can never drain.
    fn handle_peer_update(&mut self, peer: PeerId, event: PeerEvent) {
        match event {
            PeerEvent::ChainSync(PeerChainSyncEvent::RollForward(header)) => {
                let slot = header.slot;
                let hash = header.hash;
                let request_body_from = self.chain.handle_roll_forward(peer, header);
                if !request_body_from.is_empty() {
                    // Request the block from the first peer which announced it
                    self.request_block(slot, hash, request_body_from);
                }
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::RollBackward(point)) => {
                self.chain.handle_roll_backward(peer, point);
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::IntersectNotFound(tip)) => {
                // We called find_intersect on a peer, and it didn't recognize any of the points we passed.
                // That peer must either be behind us or on a different fork; either way, that chain should sync from its own tip
                if let Some(peer) = self.peers.get(&peer) {
                    peer.find_intersect(vec![tip]);
                }
            }
            PeerEvent::BlockFetched(fetched) => {
                for peer in self.peers.values_mut() {
                    peer.ack_block(fetched.hash);
                }
                self.chain.handle_body_fetched(fetched.slot, fetched.hash, fetched.body);
            }
            PeerEvent::Disconnected => {
                self.handle_disconnect(peer);
            }
        }
    }

    fn handle_disconnect(&mut self, id: PeerId) {
        let Some(peer) = self.peers.remove(&id) else {
            return;
        };
        warn!("disconnected from {}", peer.conn.address);
        let was_preferred = self.chain.preferred_upstream.is_some_and(|i| i == id);
        if was_preferred {
            if let Some(new_preferred) = self.peers.keys().next().copied() {
                self.set_preferred_upstream(new_preferred);
            } else {
                warn!("no upstream peers!");
                self.clear_preferred_upstream();
            }
        }
        for (requested_hash, requested_slot) in peer.reqs {
            let announcers = self.chain.block_announcers(requested_slot, requested_hash);
            self.request_block(requested_slot, requested_hash, announcers);
        }

        let address = peer.conn.address.clone();
        self.handle_new_connection(address, Duration::from_secs(5));
    }

    fn request_block(&mut self, slot: u64, hash: BlockHash, announcers: Vec<PeerId>) {
        for announcer in announcers {
            let Some(peer) = self.peers.get_mut(&announcer) else {
                continue;
            };
            if peer.request_block(hash, slot) {
                break; // only fetch from one
            } else {
                self.handle_disconnect(announcer);
            }
        }
    }

    fn set_preferred_upstream(&mut self, id: PeerId) {
        let Some(peer) = self.peers.get(&id) else {
            warn!("setting preferred upstream to unrecognized node {id:?}");
            return;
        };
        info!("setting preferred upstream to {}", peer.conn.address);
        self.chain.handle_new_preferred_upstream(id);
    }

    fn clear_preferred_upstream(&mut self) {
        self.chain.clear_preferred_upstream();
    }

    async fn publish_blocks(&mut self) -> Result<()> {
        while let Some((header, body, rolled_back)) = self.chain.next_unpublished_block() {
            self.block_sink.announce(header, body, rolled_back).await?;
            self.published_blocks += 1;
            if self.published_blocks.is_multiple_of(100) {
                debug!("Published block {}", header.number);
            }
            self.chain.handle_block_published();
        }
        Ok(())
    }
}

pub enum NetworkEvent {
    PeerUpdate { peer: PeerId, event: PeerEvent },
    SyncPointUpdate { point: Point },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(pub(crate) u64);

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
