use std::{
    collections::{BTreeMap, HashMap, VecDeque},
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
    security_param: u64,
    next_id: u64,
    peers: BTreeMap<PeerId, PeerConnection>,
    preferred_upstream: Option<PeerId>,
    blocks_to_fetch: VecDeque<Header>,
    blocks: HashMap<BlockHash, BlockStatus>,
    chain_prefix: VecDeque<Point>,
    rolled_back: bool,
    events: mpsc::Receiver<NetworkEvent>,
    events_sender: mpsc::Sender<NetworkEvent>,
    block_sink: BlockSink,
    published_blocks: u64,
}

impl NetworkManager {
    pub fn new(
        network_magic: u64,
        security_param: u64,
        events: mpsc::Receiver<NetworkEvent>,
        events_sender: mpsc::Sender<NetworkEvent>,
        block_sink: BlockSink,
    ) -> Self {
        Self {
            network_magic,
            security_param,
            next_id: 0,
            peers: BTreeMap::new(),
            preferred_upstream: None,
            blocks_to_fetch: VecDeque::new(),
            blocks: HashMap::new(),
            chain_prefix: VecDeque::new(),
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
                    let maybe_publish_blocks = self.handle_peer_update(peer, event);
                    if maybe_publish_blocks {
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
        if self.preferred_upstream.is_some() {
            let points = self.choose_points_for_find_intersect();
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
            let Some(upstream) = self.preferred_upstream else {
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
    fn handle_peer_update(&mut self, peer: PeerId, event: PeerEvent) -> bool {
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
                                if let Err(e) = peer.request_block(header.hash, header.slot) {
                                    warn!("could not request block from {}: {e}", peer.address);
                                    self.handle_disconnect(announcer);
                                }
                                break; // only fetch from one
                            }
                        }
                        false
                    }
                    BlockStatus::Fetched(_) => {
                        // If chainsync has requested a block which we've already fetched,
                        // we might be able to publish one or more.
                        is_preferred
                    }
                }
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::RollBackward(point)) => {
                if is_preferred {
                    match point {
                        Point::Origin => {
                            self.rolled_back = !self.chain_prefix.is_empty();
                            self.chain_prefix.clear();
                            self.blocks_to_fetch.clear();
                        }
                        Point::Specific(slot, _) => {
                            // don't bother fetching any blocks from after the rollback point
                            while self.blocks_to_fetch.back().is_some_and(|b| b.slot > slot) {
                                self.blocks_to_fetch.pop_back();
                            }

                            // If we're rolling back to before a block which we've emitted events for,
                            // set rolled_back to true so that we signal that in the next message.
                            while self
                                .chain_prefix
                                .back()
                                .is_some_and(|point| is_point_after(point, slot))
                            {
                                self.chain_prefix.pop_back();
                                self.rolled_back = true;
                            }
                        }
                    }
                }
                false
            }
            PeerEvent::BlockFetched(fetched) => {
                let Some(block) = self.blocks.get_mut(&fetched.hash) else {
                    return false;
                };
                block.set_body(&fetched.body);
                true
            }
            PeerEvent::Disconnected => {
                self.handle_disconnect(peer);
                false
            }
        }
    }

    fn handle_disconnect(&mut self, id: PeerId) {
        let Some(peer) = self.peers.remove(&id) else {
            return;
        };
        warn!("disconnected from {}", peer.address);
        let was_preferred = self.preferred_upstream.is_some_and(|i| i == id);
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
        self.preferred_upstream = Some(id);

        // If our preferred upstream changed, resync all connections.
        // That will trigger a rollback if needed.
        let points = self.choose_points_for_find_intersect();
        for peer in self.peers.values() {
            if let Err(error) = peer.find_intersect(points.clone()) {
                warn!("could not sync {}: {error:#}", peer.address)
            }
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
            let point = Point::Specific(header.slot, header.hash.to_vec());
            self.chain_prefix.push_back(point);
            while self.chain_prefix.len() > self.security_param as usize {
                self.chain_prefix.pop_front();
            }
            self.rolled_back = false;
            self.blocks_to_fetch.pop_front();
        }
        Ok(())
    }

    fn choose_points_for_find_intersect(&self) -> Vec<Point> {
        let mut iterator = self.chain_prefix.iter().rev();
        let mut result = vec![];

        // send the 5 most recent points
        for _ in 0..5 {
            if let Some(next) = iterator.next() {
                result.push(next.clone());
            }
        }

        // then 5 more points, spaced out by 10 block heights each
        let mut iterator = iterator.step_by(10);
        for _ in 0..5 {
            if let Some(next) = iterator.next() {
                result.push(next.clone());
            }
        }

        // then 5 more points, spaced out by a total of 100 block heights each
        // (in case of an implausibly long rollback)
        let mut iterator = iterator.step_by(10);
        for _ in 0..5 {
            if let Some(next) = iterator.next() {
                result.push(next.clone());
            }
        }

        result
    }
}

const fn is_point_after(point: &Point, slot: u64) -> bool {
    match point {
        Point::Origin => false,
        Point::Specific(s, _) => *s > slot,
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
