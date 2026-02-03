use std::{collections::BTreeMap, time::Duration};

use crate::{
    BlockSink,
    block_flow::BlockFlowHandler,
    chain_state::{ChainEvent, ChainState},
    connection::{PeerChainSyncEvent, PeerConnection, PeerEvent},
};
use acropolis_common::BlockHash;
use anyhow::{Context as _, Result, bail};
use pallas::network::miniprotocols::Point;
use tokio::sync::mpsc;
use tracing::{info, warn};

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
    network_magic: u32,
    next_id: u64,
    peers: BTreeMap<PeerId, PeerData>,
    chain: ChainState,
    events: mpsc::Receiver<NetworkEvent>,
    events_sender: mpsc::Sender<NetworkEvent>,
    block_sink: BlockSink,
    published_blocks: u64,
    sync_point: Option<Point>,
    flow_handler: BlockFlowHandler,
}

impl NetworkManager {
    pub fn new(
        network_magic: u32,
        events: mpsc::Receiver<NetworkEvent>,
        events_sender: mpsc::Sender<NetworkEvent>,
        block_sink: BlockSink,
        flow_handler: BlockFlowHandler,
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
            sync_point: None,
            flow_handler,
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
                self.flow_handler.publish_pending().await?;
                self.publish_events().await?;
            }
            NetworkEvent::SyncPointUpdate { point } => {
                self.chain = ChainState::new();
                self.flow_handler.handle_sync_reset();

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
            NetworkEvent::BlockWanted { hash, slot } => {
                self.handle_block_wanted(hash, slot);
            }
        }

        Ok(())
    }

    fn handle_block_wanted(&mut self, hash: BlockHash, slot: u64) {
        let announcers = self.chain.block_announcers(slot, hash);
        if announcers.is_empty() {
            warn!("BlockWanted for unknown block {hash} at slot {slot}");
            return;
        }
        self.request_block(slot, hash, announcers);
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
        } else if let Some(sync_point) = self.sync_point.as_ref() {
            peer.find_intersect(vec![sync_point.clone()]);
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
        self.sync_point = Some(point);
    }

    // Implementation note: this method is deliberately synchronous/non-blocking.
    // The task which handles network events should only block when waiting for new messages,
    // or when publishing messages to other modules. This avoids deadlock; if our event queue
    // is full and this method is blocked on writing to it, the queue can never drain.
    fn handle_peer_update(&mut self, peer: PeerId, event: PeerEvent) {
        match event {
            PeerEvent::ChainSync(PeerChainSyncEvent::RollForward(header, tip)) => {
                self.chain.handle_tip(peer, tip);
                let slot = header.slot;
                let hash = header.hash;
                let announcers = self.chain.handle_roll_forward(peer, header.clone());

                if let Some(peers) = self.flow_handler.handle_roll_forward(&header, announcers) {
                    self.request_block(slot, hash, peers);
                }
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::RollBackward(point, tip)) => {
                self.chain.handle_tip(peer, tip);

                // Notify flow handler of rollback
                let rollback_slot = match &point {
                    Point::Origin => 0,
                    Point::Specific(slot, _) => *slot,
                };
                self.flow_handler.handle_roll_backward(rollback_slot);

                self.chain.handle_roll_backward(peer, point);
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::IntersectNotFound(tip)) => {
                self.chain.handle_tip(peer, tip.clone());
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
                // Notify flow handler that block was fetched
                self.flow_handler.handle_block_fetched(fetched.slot, fetched.hash);
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
        self.chain.handle_disconnect(id);
        if self.chain.preferred_upstream.is_none() {
            if let Some(new_preferred) = self.peers.keys().next().copied() {
                self.set_preferred_upstream(new_preferred);
            } else {
                warn!("no upstream peers!");
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

    async fn publish_events(&mut self) -> Result<()> {
        while let Some(event) = self.chain.next_unpublished_event() {
            let tip = self.chain.preferred_upstream_tip();
            match event {
                ChainEvent::RollForward { header, body } => {
                    self.block_sink.announce_roll_forward(header, body, tip).await?;
                    self.published_blocks += 1;
                    if self.published_blocks.is_multiple_of(100) {
                        info!("Published block {}", header.number);
                    }
                }
                ChainEvent::RollBackward { header } => {
                    self.block_sink.announce_roll_backward(header, tip).await?;
                }
            }
            self.chain.handle_event_published();
        }
        Ok(())
    }
}

pub enum NetworkEvent {
    PeerUpdate { peer: PeerId, event: PeerEvent },
    SyncPointUpdate { point: Point },
    BlockWanted { hash: BlockHash, slot: u64 },
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
