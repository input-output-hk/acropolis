use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

use crate::{
    BlockSink,
    block_flow::BlockFlowHandler,
    connection::{PeerChainSyncEvent, PeerConnection, PeerEvent},
    peer_manager::{PeerManager, PeerManagerConfig},
    peer_sharing::request_peers,
};
use acropolis_common::BlockHash;
use anyhow::{Context as _, Result, bail};
use pallas::network::miniprotocols::Point;
use tokio::{sync::mpsc, time};
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
    /// Cold peer set and discovery rate-limiting. `None` when `peer_sharing_enabled = false`.
    ///
    /// # TODO(warm-peers): Add `warm_peers: Option<WarmPeerManager>` here for the warm tier
    /// when the warm/hot promotion split is implemented. The warm manager would handle
    /// cold→warm promotion and warm→hot elevation independently from this hot peer set.
    ///
    /// # TODO(ledger-peers): Subscribe to `SPOStateMessage` here (or in `run()`) to receive
    /// relay addresses at epoch boundaries and forward them to `peer_manager.seed_from_ledger()`.
    pub peer_manager: Option<PeerManager>,
    min_hot_peers: usize,
}

impl NetworkManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_addresses: Vec<String>,
        network_magic: u32,
        events: mpsc::Receiver<NetworkEvent>,
        events_sender: mpsc::Sender<NetworkEvent>,
        block_sink: BlockSink,
        flow_handler: BlockFlowHandler,
        target_peer_count: usize,
        min_hot_peers: usize,
        peer_sharing_enabled: bool,
        churn_interval_secs: u64,
        peer_sharing_timeout_secs: u64,
    ) -> Self {
        let peer_manager = if peer_sharing_enabled {
            Some(PeerManager::new(PeerManagerConfig {
                target_peer_count,
                min_hot_peers,
                peer_sharing_enabled,
                churn_interval_secs,
                peer_sharing_timeout_secs,
            }))
        } else {
            None
        };

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
            peer_manager,
            min_hot_peers,
        };

        if peer_sharing_enabled {
            // Seed cold list from config (FR-002): all addresses go to cold first
            let empty_hot: HashSet<String> = HashSet::new();
            if let Some(ref mut pm) = manager.peer_manager {
                pm.seed(&node_addresses, &empty_hot);
            }
            // Connect only up to min_hot_peers initially (FR-002)
            let initial_count = node_addresses.len().min(min_hot_peers);
            for address in node_addresses.into_iter().take(initial_count) {
                // Remove from cold before connecting (it becomes hot)
                if let Some(ref mut pm) = manager.peer_manager {
                    pm.mark_as_promoted(&address);
                }
                manager.handle_new_connection(address, Duration::ZERO);
            }
        } else {
            // Disabled mode: connect all addresses immediately (pre-feature baseline, FR-010)
            for address in node_addresses {
                manager.handle_new_connection(address, Duration::ZERO);
            }
        }

        manager
    }

    /// Hardcoded discovery interval (not configurable per FR-009 — only the 5 items listed there).
    const DISCOVERY_INTERVAL: Duration = Duration::from_secs(60);

    pub async fn run(mut self) -> Result<()> {
        let churn_interval = self
            .peer_manager
            .as_ref()
            .map(|pm| Duration::from_secs(pm.config().churn_interval_secs))
            .unwrap_or(Duration::from_secs(600));

        let mut churn_ticker = time::interval(churn_interval);
        churn_ticker.tick().await; // skip the immediate first tick
        let mut discovery_ticker = time::interval(Self::DISCOVERY_INTERVAL);
        discovery_ticker.tick().await; // skip the immediate first tick

        // TODO(ledger-peers): Subscribe to `SPOStateMessage` (cardano.spo.state topic) here
        // to receive relay addresses at epoch boundaries. On each epoch message, call
        // `peer_manager.seed_from_ledger(relay_addrs, &hot_set)` (method TBD in PeerManager).
        // This requires subscribing to the message bus before entering this loop.
        loop {
            tokio::select! {
                event = self.events.recv() => {
                    match event {
                        Some(e) => self.on_network_event(e).await?,
                        None => break,
                    }
                }
                _ = churn_ticker.tick(), if self.peer_manager.is_some() => {
                    self.on_churn();
                }
                _ = discovery_ticker.tick(), if self.peer_manager.is_some() => {
                    self.on_discovery_tick();
                }
            }
        }

        Ok(())
    }

    async fn on_network_event(&mut self, event: NetworkEvent) -> Result<()> {
        match event {
            NetworkEvent::PeersDiscovered {
                from_peer: _from_peer,
                addresses,
            } => {
                let hot: HashSet<String> =
                    self.peers.values().map(|p| p.conn.address.clone()).collect();
                if let Some(ref mut pm) = self.peer_manager {
                    let count = addresses.len();
                    pm.add_discovered(addresses, &hot);
                    info!(
                        discovered = count,
                        cold_count = pm.cold_count(),
                        "peer-sharing discovery batch complete"
                    );
                }
            }
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

    /// Called when the discovery ticker fires. Selects a cooldown-eligible hot peer and
    /// spawns a peer-sharing exchange task, sending results as `PeersDiscovered` events.
    fn on_discovery_tick(&mut self) {
        let pm = match self.peer_manager.as_mut() {
            Some(pm) => pm,
            None => return,
        };
        let hot_count = self.peers.len();
        if !pm.needs_discovery(hot_count) {
            return;
        }

        // Collect cooldown-eligible hot peers
        let eligible: Vec<(PeerId, String)> = self
            .peers
            .iter()
            .filter(|(id, _)| pm.can_query(**id))
            .map(|(id, p)| (*id, p.conn.address.clone()))
            .collect();

        if eligible.is_empty() {
            return;
        }

        // Randomly select one eligible peer
        use rand::seq::IteratorRandom;
        let (peer_id, address) = eligible.into_iter().choose(&mut rand::rng()).unwrap();

        // Record query BEFORE spawning (D-006 invariant)
        pm.record_query(peer_id);

        let magic = self.network_magic;
        let amount = pm.config().target_peer_count.min(255) as u8;
        let timeout = Duration::from_secs(pm.config().peer_sharing_timeout_secs);
        let sender = self.events_sender.clone();

        tokio::spawn(async move {
            match request_peers(&address, magic, amount, timeout).await {
                Ok(addrs) => {
                    let _ = sender
                        .send(NetworkEvent::PeersDiscovered {
                            from_peer: peer_id,
                            addresses: addrs,
                        })
                        .await;
                }
                Err(e) => {
                    warn!(peer = %address, error = %e, "peer-sharing exchange failed");
                }
            }
        });
    }

    /// Called when the churn ticker fires. Demotes one randomly selected hot peer
    /// (above `min_hot_peers`) to cold and promotes a cold peer to maintain count.
    ///
    /// # TODO(warm-peers): When warm tier is added, churn should demote hot→warm first,
    /// then a separate warm→cold demotion maintains the warm pool. The `should_churn`
    /// check and peer selection logic below remain the same.
    fn on_churn(&mut self) {
        let pm = match self.peer_manager.as_mut() {
            Some(pm) => pm,
            None => return,
        };
        let hot_count = self.peers.len();
        if !pm.should_churn(hot_count) {
            return;
        }

        // Randomly select a hot peer to demote
        use rand::seq::IteratorRandom;
        let Some((victim_id, _)) = self.peers.iter().choose(&mut rand::rng()) else {
            return;
        };
        let victim_id = *victim_id;
        let Some(victim) = self.peers.remove(&victim_id) else {
            return;
        };
        let address = victim.conn.address.clone();

        // Add to cold (with cap enforcement) before logging
        let hot: HashSet<String> = self.peers.values().map(|p| p.conn.address.clone()).collect();
        if let Some(ref mut pm) = self.peer_manager {
            pm.add_discovered(vec![address.clone()], &hot);
            info!(
                address = %address,
                hot_count = self.peers.len(),
                cold_count = pm.cold_count(),
                "peer demoted hot→cold via churn"
            );
        }

        // Disconnect the peer's connection task
        self.flow_handler.handle_disconnect(victim_id, self.peers.keys().next().copied());

        // Only promote if we dropped below min_hot_peers (FR-003)
        if self.peers.len() < self.min_hot_peers {
            self.try_promote_cold_peer();
        }
    }

    /// Attempt to promote a cold peer to a hot connection.
    ///
    /// Inserts into `self.peers` at spawn time (D-012 invariant). The connection will
    /// attempt to reach the peer after `delay`; on failure the peer is disconnected and
    /// `mark_failed` is called via `handle_disconnect`.
    ///
    /// # TODO(warm-peers): When warm tier is added, this method becomes `try_promote_to_warm()`.
    /// A separate `try_promote_warm_to_hot()` method handles the warm→hot elevation after
    /// connection validation (e.g. version check, latency gate).
    fn try_promote_cold_peer(&mut self) {
        let Some(ref mut pm) = self.peer_manager else {
            return;
        };
        let Some(addr) = pm.take_cold_peer() else {
            return;
        };
        info!(
            address = %addr,
            hot_count = self.peers.len() + 1, // +1 for the peer we're about to spawn
            cold_count = pm.cold_count(),
            "promoting cold peer to hot"
        );
        self.handle_new_connection(addr, Duration::ZERO);
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

    /// Called when a hot peer disconnects. Removes from `peers`, re-routes in-flight fetches,
    /// and (when peer_manager is active) promotes a cold peer if below `min_hot_peers`.
    pub fn on_peer_disconnected(&mut self, id: PeerId) {
        let Some(peer) = self.peers.remove(&id) else {
            return;
        };
        warn!(address = %peer.conn.address, "disconnected from peer");

        // The next peer is temporary needed for Direct mode flow handler only
        self.flow_handler.handle_disconnect(id, self.peers.keys().next().copied());

        // Re-request any in-flight block fetches from remaining announcers.
        for (requested_hash, requested_slot) in peer.reqs {
            if let Some(announcers) =
                self.flow_handler.block_announcers(requested_slot, requested_hash)
            {
                self.request_block(requested_slot, requested_hash, announcers);
            }
        }

        let address = peer.conn.address.clone();

        if self.peer_manager.is_some() {
            // P2P mode: check if we need to promote a cold peer (FR-003)
            let hot_count = self.peers.len();
            if hot_count < self.min_hot_peers {
                self.try_promote_cold_peer();
            }
            // Note: also reconnect the original peer with backoff if no cold available
            // (existing behaviour: reconnect with 5s delay acts as fallback)
            // If we promoted a cold peer and still have capacity, the reconnect will add
            // a surplus hot peer; churn ticker will resolve excess at next interval.
            self.handle_new_connection(address, Duration::from_secs(5));
        } else {
            // Disabled mode: reconnect with 5s backoff (pre-feature baseline, FR-010)
            self.handle_new_connection(address, Duration::from_secs(5));
        }
    }

    fn handle_disconnect(&mut self, id: PeerId) {
        self.on_peer_disconnected(id);
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
    PeerUpdate {
        peer: PeerId,
        event: PeerEvent,
    },
    SyncPointUpdate {
        point: Point,
    },
    BlockWanted {
        hash: BlockHash,
        slot: u64,
    },
    BlockRejected {
        hash: BlockHash,
        slot: u64,
    },
    /// Addresses discovered via peer-sharing from a connected hot peer.
    PeersDiscovered {
        from_peer: PeerId,
        addresses: Vec<String>,
    }, // from_peer reserved for future filtering
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(pub u64);

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
            target_peer_count: 15,
            min_hot_peers: 3,
            peer_sharing_enabled: false,
            churn_interval_secs: 600,
            peer_sharing_timeout_secs: 10,
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
            15,
            3,
            false,
            600,
            10,
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

    // --- US1: peer promotion test ---

    #[tokio::test]
    async fn promotes_cold_peer_when_hot_drops_below_min() {
        // Build NetworkManager with peer_sharing enabled, 1 cold peer, min_hot_peers=1
        // Send PeerEvent::Disconnected for the single hot peer
        // Assert that try_promote_cold_peer was called (cold peer count drops to 0)
        let context = test_context();
        let (events_sender, events) = mpsc::channel(32);

        let cfg = InterfaceConfig {
            block_topic: "cardano.block.available".to_string(),
            sync_point: SyncPoint::Origin,
            genesis_completion_topic: "cardano.sequence.bootstrapped".to_string(),
            sync_command_topic: "cardano.sync.command".to_string(),
            node_addresses: vec!["cold.peer.example.com:3001".to_string()],
            cache_dir: std::path::PathBuf::from("/tmp"),
            genesis_values: None,
            consensus_topic: "cardano.consensus.offers".to_string(),
            block_wanted_topic: "cardano.consensus.wants".to_string(),
            target_peer_count: 15,
            min_hot_peers: 1,
            peer_sharing_enabled: true,
            churn_interval_secs: 600,
            peer_sharing_timeout_secs: 10,
        };

        let flow_handler = BlockFlowHandler::new(
            &cfg,
            BlockFlowMode::Consensus,
            context.clone(),
            events_sender.clone(),
        )
        .await
        .unwrap();

        let mut manager = NetworkManager::new(
            vec![],
            0,
            events,
            events_sender,
            test_sink(context),
            flow_handler,
            cfg.target_peer_count,
            cfg.min_hot_peers,
            cfg.peer_sharing_enabled,
            cfg.churn_interval_secs,
            cfg.peer_sharing_timeout_secs,
        );

        // Seed a cold peer manually
        if let Some(ref mut pm) = manager.peer_manager {
            let hot: std::collections::HashSet<String> = std::collections::HashSet::new();
            pm.seed(&["cold.peer.example.com:3001".to_string()], &hot);
        }
        let cold_before = manager.peer_manager.as_ref().map(|pm| pm.cold_count()).unwrap_or(0);
        assert_eq!(cold_before, 1, "should have 1 cold peer before promotion");

        // Add a fake hot peer so disconnect triggers promotion
        let hot_peer = PeerId(100);
        add_test_peer_with_address(&mut manager, hot_peer, "hot.peer.example.com:3001");

        // Simulate disconnect: remove hot peer, triggering promotion
        manager.on_peer_disconnected(hot_peer);

        let cold_after = manager.peer_manager.as_ref().map(|pm| pm.cold_count()).unwrap_or(0);
        assert_eq!(
            cold_after, 0,
            "cold peer should have been promoted after disconnect"
        );
    }

    fn add_test_peer_with_address(manager: &mut NetworkManager, peer: PeerId, address: &str) {
        let sender = PeerMessageSender {
            sink: manager.events_sender.clone(),
            id: peer,
        };
        let conn = PeerConnection::new(address.to_string(), 0, sender, Duration::from_secs(3600));
        manager.peers.insert(peer, PeerData::new(conn));
    }

    // --- FR-010: disabled mode test ---

    #[tokio::test]
    async fn disabled_mode_skips_all_discovery() {
        let context = test_context();
        let (events_sender, events) = mpsc::channel(32);

        let cfg = InterfaceConfig {
            block_topic: "cardano.block.available".to_string(),
            sync_point: SyncPoint::Origin,
            genesis_completion_topic: "cardano.sequence.bootstrapped".to_string(),
            sync_command_topic: "cardano.sync.command".to_string(),
            node_addresses: vec![],
            cache_dir: std::path::PathBuf::from("/tmp"),
            genesis_values: None,
            consensus_topic: "cardano.consensus.offers".to_string(),
            block_wanted_topic: "cardano.consensus.wants".to_string(),
            target_peer_count: 15,
            min_hot_peers: 3,
            peer_sharing_enabled: false, // disabled
            churn_interval_secs: 600,
            peer_sharing_timeout_secs: 10,
        };

        let flow_handler = BlockFlowHandler::new(
            &cfg,
            BlockFlowMode::Consensus,
            context.clone(),
            events_sender.clone(),
        )
        .await
        .unwrap();

        let manager = NetworkManager::new(
            vec![],
            0,
            events,
            events_sender,
            test_sink(context),
            flow_handler,
            cfg.target_peer_count,
            cfg.min_hot_peers,
            cfg.peer_sharing_enabled,
            cfg.churn_interval_secs,
            cfg.peer_sharing_timeout_secs,
        );

        assert!(
            manager.peer_manager.is_none(),
            "peer_manager must be None when peer_sharing_enabled=false"
        );
        assert_eq!(
            manager.peers.len(),
            0,
            "no peers connected with empty node_addresses and disabled mode"
        );
    }

    // --- US2: peers discovered event test ---

    #[tokio::test]
    async fn peers_discovered_event_adds_to_cold_list() {
        let context = test_context();
        let (events_sender, events) = mpsc::channel(32);

        let cfg = InterfaceConfig {
            block_topic: "cardano.block.available".to_string(),
            sync_point: SyncPoint::Origin,
            genesis_completion_topic: "cardano.sequence.bootstrapped".to_string(),
            sync_command_topic: "cardano.sync.command".to_string(),
            node_addresses: vec![],
            cache_dir: std::path::PathBuf::from("/tmp"),
            genesis_values: None,
            consensus_topic: "cardano.consensus.offers".to_string(),
            block_wanted_topic: "cardano.consensus.wants".to_string(),
            target_peer_count: 15,
            min_hot_peers: 3,
            peer_sharing_enabled: true,
            churn_interval_secs: 600,
            peer_sharing_timeout_secs: 10,
        };

        let flow_handler = BlockFlowHandler::new(
            &cfg,
            BlockFlowMode::Consensus,
            context.clone(),
            events_sender.clone(),
        )
        .await
        .unwrap();

        let mut manager = NetworkManager::new(
            vec![],
            0,
            events,
            events_sender,
            test_sink(context),
            flow_handler,
            cfg.target_peer_count,
            cfg.min_hot_peers,
            cfg.peer_sharing_enabled,
            cfg.churn_interval_secs,
            cfg.peer_sharing_timeout_secs,
        );

        let addresses = vec![
            "185.1.2.3:3001".to_string(),
            "185.4.5.6:3001".to_string(),
            "185.7.8.9:3001".to_string(),
        ];
        manager
            .on_network_event(NetworkEvent::PeersDiscovered {
                from_peer: PeerId(1),
                addresses,
            })
            .await
            .unwrap();

        let cold = manager.peer_manager.as_ref().map(|pm| pm.cold_count()).unwrap_or(0);
        assert_eq!(
            cold, 3,
            "PeersDiscovered must add valid addresses to cold set"
        );
    }

    // --- US3: churn tests ---

    #[tokio::test]
    async fn churn_demotes_random_hot_peer_above_min() {
        let context = test_context();
        let (events_sender, events) = mpsc::channel(32);

        let cfg = InterfaceConfig {
            block_topic: "cardano.block.available".to_string(),
            sync_point: SyncPoint::Origin,
            genesis_completion_topic: "cardano.sequence.bootstrapped".to_string(),
            sync_command_topic: "cardano.sync.command".to_string(),
            node_addresses: vec![],
            cache_dir: std::path::PathBuf::from("/tmp"),
            genesis_values: None,
            consensus_topic: "cardano.consensus.offers".to_string(),
            block_wanted_topic: "cardano.consensus.wants".to_string(),
            target_peer_count: 15,
            min_hot_peers: 2,
            peer_sharing_enabled: true,
            churn_interval_secs: 600,
            peer_sharing_timeout_secs: 10,
        };

        let flow_handler = BlockFlowHandler::new(
            &cfg,
            BlockFlowMode::Consensus,
            context.clone(),
            events_sender.clone(),
        )
        .await
        .unwrap();

        let mut manager = NetworkManager::new(
            vec![],
            0,
            events,
            events_sender,
            test_sink(context),
            flow_handler,
            cfg.target_peer_count,
            cfg.min_hot_peers,
            cfg.peer_sharing_enabled,
            cfg.churn_interval_secs,
            cfg.peer_sharing_timeout_secs,
        );

        // Add 4 hot peers
        for i in 1u64..=4 {
            add_test_peer_with_address(&mut manager, PeerId(i), &format!("10.0.0.{}:3001", i));
        }
        assert_eq!(manager.peers.len(), 4);
        manager.on_churn();
        // One peer demoted (no cold peer available to promote), so hot_count = 3
        assert_eq!(manager.peers.len(), 3, "churn must demote exactly one peer");
    }

    #[tokio::test]
    async fn churn_does_not_demote_at_min_hot_peers() {
        let context = test_context();
        let (events_sender, events) = mpsc::channel(32);

        let cfg = InterfaceConfig {
            block_topic: "cardano.block.available".to_string(),
            sync_point: SyncPoint::Origin,
            genesis_completion_topic: "cardano.sequence.bootstrapped".to_string(),
            sync_command_topic: "cardano.sync.command".to_string(),
            node_addresses: vec![],
            cache_dir: std::path::PathBuf::from("/tmp"),
            genesis_values: None,
            consensus_topic: "cardano.consensus.offers".to_string(),
            block_wanted_topic: "cardano.consensus.wants".to_string(),
            target_peer_count: 15,
            min_hot_peers: 3,
            peer_sharing_enabled: true,
            churn_interval_secs: 600,
            peer_sharing_timeout_secs: 10,
        };

        let flow_handler = BlockFlowHandler::new(
            &cfg,
            BlockFlowMode::Consensus,
            context.clone(),
            events_sender.clone(),
        )
        .await
        .unwrap();

        let mut manager = NetworkManager::new(
            vec![],
            0,
            events,
            events_sender,
            test_sink(context),
            flow_handler,
            cfg.target_peer_count,
            cfg.min_hot_peers,
            cfg.peer_sharing_enabled,
            cfg.churn_interval_secs,
            cfg.peer_sharing_timeout_secs,
        );

        // Add exactly min_hot_peers = 3 peers
        for i in 1u64..=3 {
            add_test_peer_with_address(&mut manager, PeerId(i), &format!("10.0.0.{}:3001", i));
        }
        assert_eq!(manager.peers.len(), 3);
        manager.on_churn();
        assert_eq!(
            manager.peers.len(),
            3,
            "churn must not demote at min_hot_peers"
        );
    }

    // --- Existing tests ---

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
