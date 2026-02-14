use std::{collections::BTreeMap, time::Duration};

use crate::{
    BlockSink,
    block_flow::BlockFlowHandler,
    connection::{PeerChainSyncEvent, PeerConnection, PeerEvent},
};
use acropolis_common::BlockHash;
use anyhow::{Context as _, Result, bail};
use pallas::network::miniprotocols::Point;
use tokio::sync::mpsc;
use tracing::warn;

struct PeerData {
    conn: PeerConnection,
    reqs: Vec<(BlockHash, u64)>,
}

impl PeerData {
    fn new(conn: PeerConnection) -> Self {
        Self { conn, reqs: vec![] }
    }

    fn find_intersect(&self, points: Vec<Point>) {
        if points.is_empty() {
            return;
        }
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
                self.flow_handler.publish(&mut self.block_sink, &mut self.published_blocks).await?;
            }
            NetworkEvent::SyncPointUpdate { point } => {
                self.flow_handler.handle_sync_reset();

                for peer in self.peers.values_mut() {
                    peer.reqs.clear();
                }

                if let Point::Specific(slot, _) = point {
                    let (epoch, _) = self.block_sink.genesis_values.slot_to_epoch(slot);
                    self.block_sink.last_epoch = Some(epoch);
                }

                // TODO: Temporary no-op for consensus mode
                self.flow_handler
                    .set_preferred_upstream(self.peers.iter().next().map(|(peer_id, _)| *peer_id));

                self.sync_to_point(point);
            }
            NetworkEvent::BlockWanted { hash, slot } => {
                if let Some(announcers) = self.flow_handler.block_announcers(slot, hash) {
                    self.request_block(slot, hash, announcers);
                } else {
                    warn!("BlockWanted for unknown block {hash} at slot {slot}");
                }
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
        let points = self.flow_handler.handle_new_connection(id, self.sync_point.as_ref());
        peer.find_intersect(points);
        self.peers.insert(id, peer);
    }

    pub async fn sync_to_tip(&mut self) -> Result<()> {
        loop {
            let Some(upstream) = self.flow_handler.preferred_upstream() else {
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
                self.flow_handler.handle_tip(peer, tip);
                let slot = header.slot;
                let hash = header.hash;

                // TODO: Temporary. In Direct mode returns announcers then fetches blocks, in
                // Consensus mode: returns None, so no block fetch
                if let Some(peers) = self.flow_handler.handle_roll_forward(peer, header) {
                    self.request_block(slot, hash, peers);
                }
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::RollBackward(point, tip)) => {
                self.flow_handler.handle_tip(peer, tip);
                self.flow_handler.handle_roll_backward(peer, point);
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::IntersectNotFound(tip)) => {
                self.flow_handler.handle_tip(peer, tip.clone());
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
                self.flow_handler.handle_block_fetched(fetched.slot, fetched.hash, fetched.body);
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

        // The next peer is temporary needed for Direct mode flow handler only
        self.flow_handler.handle_disconnect(id, self.peers.keys().next().copied());

        for (requested_hash, requested_slot) in peer.reqs {
            if let Some(announcers) =
                self.flow_handler.block_announcers(requested_slot, requested_hash)
            {
                // TODO: Temporary for direct mode.
                self.request_block(requested_slot, requested_hash, announcers);
            }
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
