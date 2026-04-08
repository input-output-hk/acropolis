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
use tracing::{debug, info, warn};

struct PeerData {
    conn: PeerConnection,
    reqs: Vec<(BlockHash, u64)>,
    /// True once any protocol event has been received from this peer (ChainSync, BlockFetch,
    /// etc.). Used to distinguish a cold-promoted peer that never managed to connect from
    /// one that ran successfully and then disconnected.
    established: bool,
}

impl PeerData {
    fn new(conn: PeerConnection) -> Self {
        Self {
            conn,
            reqs: vec![],
            established: false,
        }
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
    pub peer_manager: Option<PeerManager>,
    min_hot_peers: usize,
    /// PeerIds of peers that were promoted from the cold list via `try_promote_cold_peer`.
    /// Used in `on_peer_disconnected` to distinguish cold-promoted peers from initially configured connections.
    cold_origin: HashSet<PeerId>,
    /// Addresses from the static `node_addresses` config. Configured peers are always
    /// retried on disconnect and are never blacklisted.
    configured_addrs: HashSet<String>,
    connect_timeout: Duration,
    ipv6_enabled: bool,
    allow_non_public_peer_addrs: bool,
    discovery_interval: Duration,
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
        connect_timeout_secs: u64,
        ipv6_enabled: bool,
        allow_non_public_peer_addrs: bool,
        discovery_interval_secs: u64,
        peer_sharing_cooldown_secs: u64,
    ) -> Self {
        let peer_manager = if peer_sharing_enabled {
            Some(PeerManager::new(PeerManagerConfig {
                target_peer_count,
                min_hot_peers,
                peer_sharing_enabled,
                churn_interval_secs,
                peer_sharing_timeout_secs,
                peer_sharing_cooldown_secs,
            }))
        } else {
            None
        };

        let configured_addrs: HashSet<String> = node_addresses.iter().cloned().collect();

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
            cold_origin: HashSet::new(),
            configured_addrs,
            connect_timeout: Duration::from_secs(connect_timeout_secs),
            ipv6_enabled,
            allow_non_public_peer_addrs,
            discovery_interval: Duration::from_secs(discovery_interval_secs),
        };

        if peer_sharing_enabled {
            // Seed cold list from config, all addresses go to cold first
            let empty_hot: HashSet<String> = HashSet::new();
            if let Some(ref mut pm) = manager.peer_manager {
                pm.seed(&node_addresses, &empty_hot);
            }
            // Connect only up to min_hot_peers initially. These initial
            // connections bypass cold_origin tracking — they are configured peers
            // and are always retried on disconnect.
            let initial_count = node_addresses.len().min(min_hot_peers);
            for address in node_addresses.into_iter().take(initial_count) {
                // Remove from cold before connecting (it becomes hot)
                if let Some(ref mut pm) = manager.peer_manager {
                    pm.mark_as_promoted(&address);
                }
                manager.handle_new_connection(address, Duration::ZERO);
            }
        } else {
            // Disabled mode: connect all addresses immediately
            for address in node_addresses {
                manager.handle_new_connection(address, Duration::ZERO);
            }
        }

        manager
    }

    pub async fn run(mut self) -> Result<()> {
        let churn_interval = self
            .peer_manager
            .as_ref()
            .map(|pm| Duration::from_secs(pm.config().churn_interval_secs))
            .unwrap_or(Duration::from_secs(600)); // default to 10 minutes if not configured

        let mut churn_ticker = time::interval(churn_interval);
        churn_ticker.tick().await; // skip the immediate first tick
        let mut discovery_ticker = time::interval(self.discovery_interval);
        discovery_ticker.tick().await; // skip the immediate first tick

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
                from_peer,
                addresses,
            } => {
                let hot: HashSet<String> =
                    self.peers.values().map(|p| p.conn.address.clone()).collect();
                if let Some(ref mut pm) = self.peer_manager {
                    let received = addresses.len();
                    let queried_peer = self
                        .peers
                        .get(&from_peer)
                        .map(|p| p.conn.address.as_str())
                        .unwrap_or("unknown");
                    let added = pm.add_discovered(addresses, &hot);
                    info!(
                        queried_peer,
                        received,
                        added,
                        cold_count = pm.cold_count(),
                        "peer-sharing discovery batch complete"
                    );
                }
                // Promote cold peers to fill up to min_hot_peers.
                while self.peers.len() < self.min_hot_peers {
                    if !self.try_promote_cold_peer() {
                        break;
                    }
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
            NetworkEvent::SecurityParamUpdate { k } => {
                self.flow_handler.update_security_param(k);
            }
        }

        Ok(())
    }

    /// Called when the discovery ticker fires. Selects a cooldown-eligible hot peer and
    /// spawns a peer-sharing exchange task.
    fn on_discovery_tick(&mut self) {
        let pm = match self.peer_manager.as_mut() {
            Some(pm) => pm,
            None => return,
        };
        let hot_count = self.peers.len();
        if !pm.needs_discovery(hot_count) {
            let cold_count = pm.cold_count();
            if hot_count == 0 {
                info!(
                    hot_count,
                    cold_count, "discovery tick: no hot peers available, skipping"
                );
            } else {
                info!(
                    hot_count,
                    cold_count,
                    "discovery tick: peer sharing disabled or discovery not needed, skipping"
                );
            }
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
            debug!(
                hot_count,
                cold_count = pm.cold_count(),
                "discovery tick: no cooldown-eligible peers to query"
            );
            return;
        }

        // Randomly select one eligible peer
        use rand::seq::IteratorRandom;
        let (peer_id, address) = eligible.into_iter().choose(&mut rand::rng()).unwrap();

        // Record query BEFORE spawning
        pm.record_query(peer_id);

        let magic = self.network_magic;
        let amount = pm.config().target_peer_count.min(255) as u8;
        let timeout = Duration::from_secs(pm.config().peer_sharing_timeout_secs);
        let sender = self.events_sender.clone();
        let ipv6 = self.ipv6_enabled;
        let allow_non_public = self.allow_non_public_peer_addrs;

        info!(
            peer = %address,
            requesting = amount,
            hot_count,
            cold_count = pm.cold_count(),
            "discovery tick: querying peer for peer-sharing"
        );

        tokio::spawn(async move {
            match request_peers(&address, magic, amount, timeout, ipv6, allow_non_public).await {
                Ok(addrs) => {
                    info!(
                        peer = %address,
                        received = addrs.len(),
                        "peer-sharing response received"
                    );
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
    fn on_churn(&mut self) {
        let hot_count = self.peers.len();
        let replacement = {
            let pm = match self.peer_manager.as_mut() {
                Some(pm) => pm,
                None => return,
            };
            if !pm.should_churn(hot_count) {
                return;
            }
            pm.take_cold_peer()
        };

        if replacement.is_none() {
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
        self.cold_origin.remove(&victim_id); // clear before ghost disconnect fires
        let address = victim.conn.address.clone();

        // Return to cold, bypassing the failed_peers blacklist
        // since a currently hot peer must not be silently discarded.
        let hot: HashSet<String> = self.peers.values().map(|p| p.conn.address.clone()).collect();
        if let Some(ref mut pm) = self.peer_manager {
            pm.demote_to_cold(address.clone(), &hot);
            info!(
                address = %address,
                hot_count = self.peers.len(),
                cold_count = pm.cold_count(),
                "peer demoted hot→cold via churn"
            );
        }

        // Disconnect the peer's connection task
        self.flow_handler.handle_disconnect(victim_id, self.peers.keys().next().copied());

        self.rerequest_inflight(victim.reqs);

        if let Some(address) = replacement {
            self.promote_reserved_cold_peer(address);
        }
    }

    fn try_promote_cold_peer(&mut self) -> bool {
        let Some(ref mut pm) = self.peer_manager else {
            return false;
        };
        let Some(addr) = pm.take_cold_peer() else {
            return false;
        };
        self.promote_reserved_cold_peer(addr);
        true
    }

    fn promote_reserved_cold_peer(&mut self, addr: String) {
        let cold_count = self.peer_manager.as_ref().map(|pm| pm.cold_count()).unwrap_or(0);
        info!(
            address = %addr,
            hot_count = self.peers.len() + 1, // +1 for the peer we're about to spawn
            cold_count,
            "promoting cold peer to hot"
        );
        let new_id = self.handle_new_connection(addr, Duration::ZERO);
        self.cold_origin.insert(new_id);
    }

    pub fn handle_new_connection(&mut self, address: String, delay: Duration) -> PeerId {
        let id = PeerId(self.next_id);
        self.next_id += 1;
        let sender = PeerMessageSender {
            sink: self.events_sender.clone(),
            id,
        };
        let conn = PeerConnection::new(
            address,
            self.network_magic,
            sender,
            delay,
            self.connect_timeout,
        );
        let peer = PeerData::new(conn);
        let points = self.flow_handler.handle_new_connection(id, self.sync_point.as_ref());
        peer.find_intersect(points);
        self.peers.insert(id, peer);
        id
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
        // Mark established on any protocol event so we can distinguish a cold-promoted
        // peer that never managed to connect from one that ran and then disconnected.
        if !matches!(event, PeerEvent::Disconnected)
            && let Some(p) = self.peers.get_mut(&peer)
        {
            p.established = true;
        }

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
            // Ghost disconnect: peer was already removed (e.g. churn dropped the PeerData,
            // which caused the worker to exit and emit a Disconnected event).  Safe to
            // ignore — flow_handler was already called in on_churn.
            debug!(
                peer_id = id.0,
                "ignoring ghost disconnect for already-removed peer"
            );
            return;
        };
        warn!(address = %peer.conn.address, "disconnected from peer");

        // Capture state before peer is partially consumed.
        let is_cold_origin = self.cold_origin.remove(&id);
        let is_configured = self.configured_addrs.contains(&peer.conn.address);
        let established = peer.established;
        let address = peer.conn.address.clone();

        // The next peer is temporary needed for Direct mode flow handler only
        self.flow_handler.handle_disconnect(id, self.peers.keys().next().copied());

        self.rerequest_inflight(peer.reqs);

        if self.peer_manager.is_none() {
            // Disabled mode: reconnect with 5s backoff
            self.handle_new_connection(address, Duration::from_secs(5));
            return;
        }

        // P2P mode.

        if is_cold_origin && !established && !is_configured {
            // Cold-promoted peer that never established a connection (TCP refused / timeout).
            // Blacklist so peer-sharing cannot re-add it this session.
            warn!(
                address = %address,
                "cold-promoted peer never connected — blacklisting for session"
            );
            if let Some(ref mut pm) = self.peer_manager {
                pm.mark_failed(address);
            }
            // Fill the vacancy if below minimum.
            if self.peers.len() < self.min_hot_peers {
                let _ = self.try_promote_cold_peer();
            }
            return;
        }

        // Configured peers keep the existing reconnect loop even when a cold peer is
        // promoted to replace them. Discovered peers return to cold so they can be
        // re-promoted later.
        let needs_promotion = self.peers.len() < self.min_hot_peers;
        let promoted = needs_promotion && self.try_promote_cold_peer();
        if promoted {
            if is_configured {
                self.handle_new_connection(address, Duration::from_secs(5));
            } else {
                let hot: HashSet<String> =
                    self.peers.values().map(|p| p.conn.address.clone()).collect();
                if let Some(ref mut pm) = self.peer_manager {
                    pm.demote_to_cold(address, &hot);
                }
            }
        } else {
            // Cold list empty, not below minimum, or not P2P mode — reconnect directly.
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

    fn rerequest_inflight(&mut self, reqs: Vec<(BlockHash, u64)>) {
        for (hash, slot) in reqs {
            if let Some(announcers) = self.flow_handler.block_announcers(slot, hash) {
                self.request_block(slot, hash, announcers);
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
    },
    /// Updated security parameter k from protocol parameters.
    SecurityParamUpdate {
        k: u64,
    },
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
            topic: "test.block.available".to_string(),
            genesis_values: GenesisValues::mainnet(),
            upstream_cache: None,
            last_epoch: None,
            era: None,
            rolled_back: false,
        }
    }

    fn default_test_cfg() -> InterfaceConfig {
        InterfaceConfig {
            block_topic: "test.block.available".to_string(),
            sync_point: SyncPoint::Origin,
            genesis_completion_topic: "test.sequence.bootstrapped".to_string(),
            sync_command_topic: "test.sync.command".to_string(),
            node_addresses: vec![],
            cache_dir: PathBuf::from("/tmp"),
            genesis_values: None,
            protocol_params_topic: "test.protocol.parameters".to_string(),
            consensus_topic: "test.consensus.offers".to_string(),
            block_wanted_topic: "test.consensus.wants".to_string(),
            target_peer_count: 15,
            min_hot_peers: 3,
            peer_sharing_enabled: true,
            churn_interval_secs: 600,
            peer_sharing_timeout_secs: 10,
            connect_timeout_secs: 15,
            ipv6_enabled: false,
            allow_non_public_peer_addrs: true,
            discovery_interval_secs: 0,
            peer_sharing_cooldown_secs: 0,
        }
    }

    async fn test_manager_from_cfg(cfg: InterfaceConfig) -> NetworkManager {
        let context = test_context();
        let (events_sender, events) = mpsc::channel(32);
        let flow_handler = BlockFlowHandler::new(
            &cfg,
            BlockFlowMode::Consensus,
            context.clone(),
            events_sender.clone(),
        )
        .await
        .unwrap();
        NetworkManager::new(
            cfg.node_addresses.clone(),
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
            cfg.connect_timeout_secs,
            cfg.ipv6_enabled,
            cfg.allow_non_public_peer_addrs,
            cfg.discovery_interval_secs,
            cfg.peer_sharing_cooldown_secs,
        )
    }

    async fn test_consensus_manager() -> NetworkManager {
        test_manager_from_cfg(InterfaceConfig {
            peer_sharing_enabled: false,
            ..default_test_cfg()
        })
        .await
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
            Duration::from_secs(15),
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
    async fn promotes_cold_peer_when_hot_drops_below_min() {
        // Build NetworkManager with peer_sharing enabled, 1 cold peer, min_hot_peers=1
        // Send PeerEvent::Disconnected for the single hot peer
        // Assert that try_promote_cold_peer was called (cold peer count drops to 0)
        let mut manager = test_manager_from_cfg(InterfaceConfig {
            min_hot_peers: 1,
            ..default_test_cfg()
        })
        .await;

        // Seed a cold peer manually (not via node_addresses so no auto-connect happens)
        if let Some(ref mut pm) = manager.peer_manager {
            let hot: HashSet<String> = HashSet::new();
            pm.seed(&["cold.peer.example.com:3001".to_string()], &hot);
        }
        let cold_before = manager.peer_manager.as_ref().map(|pm| pm.cold_count()).unwrap_or(0);
        assert_eq!(cold_before, 1, "should have 1 cold peer before promotion");

        // Add a fake hot peer so disconnect triggers promotion
        let hot_peer = PeerId(100);
        add_test_peer_with_address(&mut manager, hot_peer, "hot.peer.example.com:3001");

        // Simulate disconnect: remove hot peer, triggering promotion
        manager.on_peer_disconnected(hot_peer);

        // The original cold peer should have been promoted (no longer in cold).
        // The disconnected hot peer should have been returned to cold so it stays in rotation.
        let pm = manager.peer_manager.as_ref().unwrap();
        assert!(
            !pm.contains_cold("cold.peer.example.com:3001"),
            "original cold peer should have been promoted (removed from cold)"
        );
        assert!(
            pm.contains_cold("hot.peer.example.com:3001"),
            "disconnected hot peer should be returned to cold to stay in rotation"
        );
        assert_eq!(
            pm.cold_count(),
            1,
            "net cold count: promoted one, returned one"
        );
    }

    #[tokio::test]
    async fn configured_peer_reconnects_even_when_cold_peer_is_promoted() {
        let mut manager = test_manager_from_cfg(InterfaceConfig {
            node_addresses: vec!["hot.peer.example.com:3001".to_string()],
            min_hot_peers: 1,
            ..default_test_cfg()
        })
        .await;

        if let Some(ref mut pm) = manager.peer_manager {
            let hot: HashSet<String> = HashSet::new();
            pm.seed(&["cold.peer.example.com:3001".to_string()], &hot);
        }

        assert_eq!(
            manager.peers.len(),
            1,
            "configured startup should create exactly one initial hot peer"
        );

        manager.on_peer_disconnected(PeerId(0));

        let pm = manager.peer_manager.as_ref().unwrap();
        assert!(
            !pm.contains_cold("cold.peer.example.com:3001"),
            "promoted cold peer must be removed from the cold set"
        );
        assert!(
            !pm.contains_cold("hot.peer.example.com:3001"),
            "configured peer should reconnect directly, not be returned to cold"
        );
        assert_eq!(
            manager.peers.len(),
            2,
            "disconnect should leave the promoted cold peer plus the configured reconnect"
        );
    }

    fn add_test_peer_with_address(manager: &mut NetworkManager, peer: PeerId, address: &str) {
        let sender = PeerMessageSender {
            sink: manager.events_sender.clone(),
            id: peer,
        };
        let conn = PeerConnection::new(
            address.to_string(),
            0,
            sender,
            Duration::from_secs(3600),
            Duration::from_secs(15),
        );
        manager.peers.insert(peer, PeerData::new(conn));
    }

    #[tokio::test]
    async fn disabled_mode_skips_all_discovery() {
        let manager = test_manager_from_cfg(InterfaceConfig {
            peer_sharing_enabled: false,
            ..default_test_cfg()
        })
        .await;

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

    #[tokio::test]
    async fn peers_discovered_event_adds_to_cold_list() {
        let mut manager = test_manager_from_cfg(default_test_cfg()).await;

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

        assert_eq!(
            manager.peers.len(),
            3,
            "discovered peers must be promoted to hot when below min_hot_peers"
        );
        let cold = manager.peer_manager.as_ref().map(|pm| pm.cold_count()).unwrap_or(0);
        assert_eq!(
            cold, 0,
            "all discovered peers should have been promoted out of cold"
        );
    }

    #[tokio::test]
    async fn churn_skips_rotation_when_no_cold_peer_is_available() {
        let mut manager = test_manager_from_cfg(InterfaceConfig {
            min_hot_peers: 2,
            ..default_test_cfg()
        })
        .await;

        // Add 4 hot peers
        for i in 1u64..=4 {
            add_test_peer_with_address(&mut manager, PeerId(i), &format!("10.0.0.{}:3001", i));
        }
        assert_eq!(manager.peers.len(), 4);
        manager.on_churn();
        assert_eq!(
            manager.peers.len(),
            4,
            "churn should do nothing when no cold replacement is available"
        );
    }

    #[tokio::test]
    async fn churn_replaces_hot_peer_when_cold_peer_is_available() {
        let mut manager = test_manager_from_cfg(InterfaceConfig {
            min_hot_peers: 2,
            ..default_test_cfg()
        })
        .await;

        if let Some(ref mut pm) = manager.peer_manager {
            let hot: HashSet<String> = HashSet::new();
            pm.seed(&["cold.peer.example.com:3001".to_string()], &hot);
        }

        for i in 1u64..=4 {
            add_test_peer_with_address(&mut manager, PeerId(i), &format!("10.0.0.{}:3001", i));
        }
        assert_eq!(manager.peers.len(), 4);

        manager.on_churn();

        let pm = manager.peer_manager.as_ref().unwrap();
        assert_eq!(
            manager.peers.len(),
            4,
            "churn should preserve hot count when a replacement is available"
        );
        assert!(
            !pm.contains_cold("cold.peer.example.com:3001"),
            "reserved cold peer should have been promoted"
        );
        assert_eq!(
            pm.cold_count(),
            1,
            "one hot peer should have been rotated back into cold"
        );
    }

    #[tokio::test]
    async fn churn_does_not_demote_at_min_hot_peers() {
        let mut manager = test_manager_from_cfg(default_test_cfg()).await;

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
