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
    /// Wanted blocks that could not be fetched immediately because
    /// no current announcer was available. Retried opportunistically when
    /// peer events arrive.
    pending_wanted: BTreeMap<(u64, BlockHash), ()>,
    sync_point: Option<Point>,
    flow_handler: BlockFlowHandler,
}

impl NetworkManager {
    pub fn new(
        node_addresses: Vec<String>,
        network_magic: u32,
        events: mpsc::Receiver<NetworkEvent>,
        events_sender: mpsc::Sender<NetworkEvent>,
        block_sink: BlockSink,
        flow_handler: BlockFlowHandler,
    ) -> Self {
        let mut manager = Self {
            network_magic,
            next_id: 0,
            peers: BTreeMap::new(),
            events,
            events_sender,
            block_sink,
            published_blocks: 0,
            pending_wanted: BTreeMap::new(),
            sync_point: None,
            flow_handler,
        };

        for address in node_addresses {
            manager.handle_new_connection(address, Duration::ZERO);
        }

        manager
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
                self.pending_wanted.clear();

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
                    self.pending_wanted.remove(&(slot, hash));
                } else {
                    if self.flow_handler.knows_block(slot, hash) {
                        warn!(
                            "BlockWanted for known block {hash} at slot {slot}, but no eligible announcers yet"
                        );
                    } else {
                        warn!("BlockWanted for unknown block {hash} at slot {slot}");
                    }
                    self.pending_wanted.insert((slot, hash), ());
                }
            }
            NetworkEvent::BlockRejected { hash, slot } => {
                let peers = self.flow_handler.block_rejected_announcers(hash);
                if peers.is_empty() {
                    warn!("BlockRejected for unknown block {hash} at slot {slot}");
                }
                for peer in peers {
                    self.handle_disconnect(peer);
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
                info!("peer {:?} rolled back to {:?}", peer, point);
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

        // Retry wants that were temporarily unresolved (e.g. out-of-order
        // BlockWanted vs announcement, or announcer churn during disconnects).
        self.retry_pending_wanted();
    }

    fn handle_disconnect(&mut self, id: PeerId) {
        let Some(peer) = self.peers.remove(&id) else {
            return;
        };
        warn!("disconnected from {}", peer.conn.address);

        // The next peer is temporary needed for Direct mode flow handler only
        self.flow_handler.handle_disconnect(id, self.peers.keys().next().copied());

        // Re-request any in-flight block fetches from remaining announcers.
        // Once a block has been requested, losing the serving peer must not leave that fetch
        // permanently stuck waiting for a fresh BlockWanted.
        for (requested_hash, requested_slot) in peer.reqs {
            if let Some(announcers) =
                self.flow_handler.block_announcers(requested_slot, requested_hash)
            {
                self.request_block(requested_slot, requested_hash, announcers);
            }
        }

        let address = peer.conn.address.clone();
        self.handle_new_connection(address, Duration::from_secs(5));
    }

    fn retry_pending_wanted(&mut self) {
        if self.pending_wanted.is_empty() {
            return;
        }

        let pending: Vec<(u64, BlockHash)> = self.pending_wanted.keys().copied().collect();
        for (slot, hash) in pending {
            if !self.flow_handler.knows_block(slot, hash) {
                self.pending_wanted.remove(&(slot, hash));
                continue;
            }

            if let Some(announcers) = self.flow_handler.block_announcers(slot, hash) {
                self.request_block(slot, hash, announcers);
                self.pending_wanted.remove(&(slot, hash));
            }
        }
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
    BlockRejected { hash: BlockHash, slot: u64 },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_flow::BlockFlowHandler;
    use crate::configuration::{InterfaceConfig, SyncPoint};
    use crate::connection::{Header, PeerChainSyncEvent, PeerEvent};
    use acropolis_common::configuration::BlockFlowMode;
    use acropolis_common::genesis_values::GenesisValues;
    use acropolis_common::messages::Message;
    use acropolis_common::{BlockHash, Era};
    use caryatid_sdk::Context;
    use caryatid_sdk::mock_bus::MockBus;
    use config::Config;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::watch;

    fn test_context() -> Arc<Context<Message>> {
        let config = Arc::new(Config::builder().build().unwrap());
        let bus = Arc::new(MockBus::<Message>::new(&config));
        let (_tx, rx) = watch::channel(true);
        Arc::new(Context::new(config, bus, rx))
    }

    fn test_sink(context: Arc<Context<Message>>) -> BlockSink {
        BlockSink {
            context,
            topic: "cardano.block.available".to_string(),
            genesis_values: GenesisValues::mainnet(),
            upstream_cache: None,
            last_epoch: None,
            era: None,
            rolled_back: false,
        }
    }

    async fn test_consensus_manager() -> NetworkManager {
        let context = test_context();
        let (events_sender, events) = mpsc::channel(32);

        let cfg = InterfaceConfig {
            block_topic: "cardano.block.available".to_string(),
            sync_point: SyncPoint::Origin,
            genesis_completion_topic: "cardano.sequence.bootstrapped".to_string(),
            sync_command_topic: "cardano.sync.command".to_string(),
            node_addresses: vec![],
            cache_dir: PathBuf::from("/tmp"),
            genesis_values: None,
            consensus_topic: "cardano.consensus.offers".to_string(),
            block_wanted_topic: "cardano.consensus.wants".to_string(),
        };

        let flow_handler = BlockFlowHandler::new(
            &cfg,
            BlockFlowMode::Consensus,
            context.clone(),
            events_sender.clone(),
        )
        .await
        .unwrap();

        NetworkManager::new(
            vec![],
            0,
            events,
            events_sender,
            test_sink(context),
            flow_handler,
        )
    }

    fn add_test_peer(manager: &mut NetworkManager, peer: PeerId) {
        let sender = PeerMessageSender {
            sink: manager.events_sender.clone(),
            id: peer,
        };
        // Delay prevents immediate network activity; tests drive state manually.
        let conn = PeerConnection::new(
            "test-peer:3001".to_string(),
            0,
            sender,
            Duration::from_secs(3600),
        );
        manager.peers.insert(peer, PeerData::new(conn));
    }

    fn test_header(slot: u64, number: u64, hash: BlockHash, parent_hash: BlockHash) -> Header {
        Header {
            hash,
            slot,
            number,
            bytes: vec![],
            era: Era::Conway,
            parent_hash: Some(parent_hash),
        }
    }

    #[tokio::test]
    async fn block_wanted_for_fetched_block_uses_fetched_announcers() {
        let mut manager = test_consensus_manager().await;
        let peer = PeerId(1);
        add_test_peer(&mut manager, peer);

        let slot = 100;
        let parent = BlockHash::new([1; 32]);
        let hash = BlockHash::new([2; 32]);
        let header = test_header(slot, 10, hash, parent);

        manager.flow_handler.handle_tip(peer, Point::Specific(slot, hash.to_vec()));
        let _ = manager.flow_handler.handle_roll_forward(peer, header);
        manager.flow_handler.handle_block_fetched(slot, hash, vec![1, 2, 3]);

        manager.on_network_event(NetworkEvent::BlockWanted { hash, slot }).await.unwrap();

        assert!(
            !manager.pending_wanted.contains_key(&(slot, hash)),
            "wanted must not remain pending once a fetched announcer exists"
        );

        let reqs = &manager.peers.get(&peer).unwrap().reqs;
        assert!(
            reqs.contains(&(hash, slot)),
            "peer should receive a fetch request for re-wanted fetched block"
        );
    }

    #[tokio::test]
    async fn pending_wanted_retries_after_late_announcement() {
        let mut manager = test_consensus_manager().await;
        let peer = PeerId(2);
        add_test_peer(&mut manager, peer);

        let slot = 200;
        let parent = BlockHash::new([3; 32]);
        let hash = BlockHash::new([4; 32]);
        let header = test_header(slot, 20, hash, parent);

        manager.on_network_event(NetworkEvent::BlockWanted { hash, slot }).await.unwrap();
        assert!(manager.pending_wanted.contains_key(&(slot, hash)));

        manager.handle_peer_update(
            peer,
            PeerEvent::ChainSync(PeerChainSyncEvent::RollForward(
                header,
                Point::Specific(slot, hash.to_vec()),
            )),
        );

        assert!(
            !manager.pending_wanted.contains_key(&(slot, hash)),
            "pending wanted should be cleared once announcement arrives"
        );
        let reqs = &manager.peers.get(&peer).unwrap().reqs;
        assert!(
            reqs.contains(&(hash, slot)),
            "late announcement should trigger block fetch retry"
        );
    }

    #[tokio::test]
    async fn retry_pending_wanted_evicts_unknown_blocks() {
        let mut manager = test_consensus_manager().await;
        let peer = PeerId(3);
        add_test_peer(&mut manager, peer);

        let slot = 300;
        let hash = BlockHash::new([5; 32]);

        manager.pending_wanted.insert((slot, hash), ());
        assert!(manager.pending_wanted.contains_key(&(slot, hash)));

        manager.retry_pending_wanted();

        assert!(
            !manager.pending_wanted.contains_key(&(slot, hash)),
            "stale entry for unknown block should be evicted"
        );
    }
}
