use std::{
    collections::BTreeMap,
    time::Duration,
};

use crate::{
    BlockSink, chain_state::ChainState, connection::{PeerChainSyncEvent, PeerConnection, PeerEvent}
};
use anyhow::{Context as _, Result, bail};
use pallas::network::miniprotocols::Point;
use tokio::sync::mpsc;
use tracing::{info, warn};

pub struct NetworkManager {
    network_magic: u64,
    next_id: u64,
    peers: BTreeMap<PeerId, PeerConnection>,
    chain: ChainState,
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
            peers: BTreeMap::new(),
            chain: ChainState::new(),
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
                NetworkEvent::PeerUpdate { peer, event } => {
                    self.handle_peer_update(peer, event);
                    if true {
                        self.publish_blocks().await?;
                    }
                }
            }
        }
        bail!("event sink closed")
    }

    pub fn handle_new_connection(&mut self, address: String, delay: Duration) {
        let id = PeerId(self.next_id);
        self.next_id += 1;
        let sender = PeerMessageSender {
            sink: self.events_sender.clone(),
            id,
        };
        let conn = PeerConnection::new(address, self.network_magic, sender, delay);
        if self.chain.preferred_upstream.is_some() {
            let points = self.chain.choose_points_for_find_intersect();
            if !points.is_empty()
                && let Err(error) = conn.find_intersect(points)
            {
                warn!("could not sync {}: {error:#}", conn.address);
            }
            self.peers.insert(id, conn);
        } else {
            self.peers.insert(id, conn);
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
            match peer.find_tip().await {
                Ok(point) => {
                    self.sync_to_point(point);
                    return Ok(());
                }
                Err(e) => {
                    warn!("could not fetch tip from {}: {e:#}", peer.address);
                    self.handle_disconnect(upstream);
                }
            }
        }
    }

    pub fn sync_to_point(&mut self, point: Point) {
        for peer in self.peers.values() {
            if let Err(error) = peer.find_intersect(vec![point.clone()]) {
                warn!("could not sync {}: {error:#}", peer.address);
            }
        }
    }

    // Implementation note: this method is deliberately synchronous/non-blocking.
    // The task which handles network events should only block when waiting for new messages,
    // or when publishing messages to other modules. This avoids deadlock; if our event queue
    // is full and this method is blocked on writing to it, the queue can never drain.
    // Returns true if we might have new events to publish downstream.
    fn handle_peer_update(&mut self, peer: PeerId, event: PeerEvent) {
        match event {
            PeerEvent::ChainSync(PeerChainSyncEvent::RollForward(header)) => {
                let slot = header.slot;
                let hash = header.hash;
                let request_body_from = self.chain.handle_roll_forward(peer, header);
                if !request_body_from.is_empty() {
                    // Request the block from the first peer which announced it
                    for announcer in request_body_from {
                        let Some(peer) = self.peers.get(&announcer) else {
                            continue;
                        };
                        if let Err(e) = peer.request_block(hash, slot) {
                            warn!("could not request block from {}: {e}", peer.address);
                            self.handle_disconnect(announcer);
                        }
                        break; // only fetch from one
                    }
                }
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::RollBackward(point)) => {
                let rolled_back = self.chain.handle_roll_backward(peer, point);
                if rolled_back {
                    self.rolled_back = true;
                }
            }
            PeerEvent::BlockFetched(fetched) => {
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
        warn!("disconnected from {}", peer.address);
        let was_preferred = self.chain.preferred_upstream.is_some_and(|i| i == id);
        if was_preferred && let Some(new_preferred) = self.peers.keys().next().copied() {
            self.set_preferred_upstream(new_preferred);
        }
        if self.peers.is_empty() {
            warn!("no upstream peers!");
        }
        let address = peer.address.clone();
        drop(peer);
        self.handle_new_connection(address, Duration::from_secs(5));
    }

    fn set_preferred_upstream(&mut self, id: PeerId) {
        let Some(peer) = self.peers.get(&id) else {
            warn!("setting preferred upstream to unrecognized node {id:?}");
            return;
        };
        info!("setting preferred upstream to {}", peer.address);
        self.chain.handle_new_preferred_upstream(id);

        // If our preferred upstream changed, resync all connections.
        // That will trigger a rollback if needed.
        let points = self.chain.choose_points_for_find_intersect();
        for peer in self.peers.values() {
            if let Err(error) = peer.find_intersect(points.clone()) {
                warn!("could not sync {}: {error:#}", peer.address)
            }
        }
    }

    async fn publish_blocks(&mut self) -> Result<()> {
        while let Some((header, body, rolled_back)) = self.chain.next_unpublished_block() {
            self.block_sink.announce(header, body, rolled_back).await?;
            self.published_blocks += 1;
            if self.published_blocks.is_multiple_of(1) {
                info!("Published block {}", header.number);
            }
            self.chain.handle_block_published();
        }
        Ok(())
    }
}

pub enum NetworkEvent {
    PeerUpdate { peer: PeerId, event: PeerEvent },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
