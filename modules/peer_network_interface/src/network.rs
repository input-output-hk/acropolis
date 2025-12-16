use std::{collections::BTreeMap, time::Duration};

use crate::{
    BlockSink,
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
    network_magic: u64,
    next_id: u64,
    peers: BTreeMap<PeerId, PeerData>,
    chain: ChainState,
    events: mpsc::Receiver<NetworkEvent>,
    events_sender: mpsc::Sender<NetworkEvent>,
    block_sink: BlockSink,
    published_blocks: u64,
    sync_point: Option<Point>,
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
            sync_point: None,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!(
            "NetworkManager: Starting run loop with {} peers",
            self.peers.len()
        );
        let mut event_count = 0u64;
        let mut last_log_time = std::time::Instant::now();
        loop {
            tokio::select! {
                event = self.events.recv() => {
                    let Some(event) = event else {
                        info!("NetworkManager: Event channel closed, exiting run loop");
                        break;
                    };
                    event_count += 1;
                    if event_count <= 5 || event_count % 1000 == 0 {
                        info!(
                            "NetworkManager: Received event #{}: {:?}",
                            event_count,
                            std::mem::discriminant(&event)
                        );
                    }
                    self.on_network_event(event).await?;
                    last_log_time = std::time::Instant::now();
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                    let elapsed = last_log_time.elapsed().as_secs();
                    info!(
                        "NetworkManager: Heartbeat - {} events processed, {} peers, {}s since last event, {} blocks published",
                        event_count, self.peers.len(), elapsed, self.published_blocks
                    );
                }
            }
        }
        info!(
            "NetworkManager: Run loop ended after {} events",
            event_count
        );
        Ok(())
    }

    async fn on_network_event(&mut self, event: NetworkEvent) -> Result<()> {
        match event {
            NetworkEvent::PeerUpdate { peer, event } => {
                self.handle_peer_update(peer, event);
                self.publish_events().await?;
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
                let request_body_from = self.chain.handle_roll_forward(peer, header);
                if !request_body_from.is_empty() {
                    // Request the block from the first peer which announced it
                    self.request_block(slot, hash, request_body_from);
                }
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::RollBackward(point, tip)) => {
                self.chain.handle_tip(peer, tip);
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
        let mut published_count = 0u64;
        while let Some(event) = self.chain.next_unpublished_event() {
            let tip = self.chain.preferred_upstream_tip();
            match event {
                ChainEvent::RollForward { header, body } => {
                    self.block_sink.announce_roll_forward(header, body, tip).await?;
                    self.published_blocks += 1;
                    published_count += 1;
                    if self.published_blocks <= 5 || self.published_blocks.is_multiple_of(100) {
                        info!("Published block {} (slot {})", header.number, header.slot);
                    }
                }
                ChainEvent::RollBackward { header } => {
                    info!(
                        "Publishing rollback to block {} (slot {})",
                        header.number, header.slot
                    );
                    self.block_sink.announce_roll_backward(header, tip).await?;
                }
            }
            self.chain.handle_event_published();
        }
        if published_count == 0 && self.published_blocks == 0 {
            // Only log once at startup if nothing is being published
            static LOGGED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                info!("NetworkManager: publish_events called but no events to publish yet");
            }
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
