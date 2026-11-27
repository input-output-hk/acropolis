use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

use crate::{
    BlockSink,
    block_flow::BlockFlowHandler,
    chain_state::SpecificPoint,
    connection::{HOUSEKEEPING_INTERVAL, PeerChainSyncEvent, PeerConnection, PeerEvent, header_of},
    peer_manager::{PeerManager, PeerManagerConfig},
    peer_sharing::request_peers,
};
use acropolis_common::BlockHash;
use anyhow::{Context as _, Result};
use pallas_network2::protocol::Point;
use tokio::{sync::mpsc, time};
use tracing::{debug, info, warn};

/// Blocks requested from all peers whose body has not arrived yet, from which on no peer is
/// asked for its next header. Catching up, chainsync outruns blockfetch (pallas-network2 sends
/// a peer one single-block range at a time) and every header waiting for its body stays in
/// memory (pallas' blockfetch queues, the chain state); without a cap that grows with the
/// distance to the tip. One cap for all peers together.
const MAX_PENDING_FETCHES: usize = 512;

/// Requested blocks still to arrive down to which a stopped chainsync runs again. That asks
/// every peer for its next header at once, each of which may request a block: well below the
/// cap, so that chainsync does not pause again at the next headers.
const RESUME_PENDING_FETCHES: usize = MAX_PENDING_FETCHES / 2;

/// A peer that sent no block body over a whole interval while blocks were requested from it
/// is taken as not serving them. Two housekeeping intervals: a serving peer gets its next range
/// on its own housekeeping, whose ticks are not aligned with these.
const FETCH_STALL_INTERVAL: Duration = HOUSEKEEPING_INTERVAL.saturating_mul(2);

/// What is fetched of peers, each kind with its own requests and stall check: blocks over
/// blockfetch.
trait Fetch {
    /// For logs.
    const WHAT: &'static str;

    fn requests(peer: &PeerData) -> &Pending;
    fn requests_mut(peer: &mut PeerData) -> &mut Pending;
    fn send(conn: &PeerConnection, point: &SpecificPoint) -> Result<()>;

    /// The peers to ask, in order, for a request `stalled` left unanswered: `None` if it is no
    /// longer wanted, none if it is given up on.
    fn rerequest_to(
        flow: &BlockFlowHandler,
        point: &SpecificPoint,
        stalled: PeerId,
    ) -> Option<Vec<PeerId>>;

    /// The requests taken from a stalled peer, before they go to other peers.
    fn on_stalled(_manager: &mut NetworkManager, _id: PeerId, _reqs: &mut Vec<SpecificPoint>) {}

    /// What a reply brings.
    type Reply;
    /// What a reply that serves the request brings.
    type Content;

    /// The content of a reply from `peer`; `None` if it only leads to another request to it.
    fn content(
        manager: &mut NetworkManager,
        peer: PeerId,
        point: &SpecificPoint,
        reply: Self::Reply,
    ) -> Option<Self::Content>;

    fn deliver(
        manager: &mut NetworkManager,
        peer: PeerId,
        point: SpecificPoint,
        content: Self::Content,
    );
}

struct RBs;

impl Fetch for RBs {
    const WHAT: &'static str = "block body";

    fn requests(peer: &PeerData) -> &Pending {
        &peer.reqs.rbs
    }

    fn requests_mut(peer: &mut PeerData) -> &mut Pending {
        &mut peer.reqs.rbs
    }

    fn send(conn: &PeerConnection, block: &SpecificPoint) -> Result<()> {
        conn.request_block(block.hash, block.slot)
    }

    /// Peers that announced the block, `stalled` last: a block asked for is never given up on
    /// while announced (consensus wants it once, Direct mode publishes in order), so a lone
    /// announcer is asked again. Its body is still used if it arrives late.
    fn rerequest_to(
        flow: &BlockFlowHandler,
        block: &SpecificPoint,
        stalled: PeerId,
    ) -> Option<Vec<PeerId>> {
        let mut announcers = flow.block_announcers(block.slot, block.hash)?;
        announcers.sort_by_key(|announcer| *announcer == stalled);
        Some(announcers)
    }

    fn on_stalled(manager: &mut NetworkManager, id: PeerId, reqs: &mut Vec<SpecificPoint>) {
        let Some(intersection) =
            manager.peers.get_mut(&id).and_then(|peer| peer.intersection.take())
        else {
            return;
        };
        // Only this peer was asked: chainsync goes on without that header.
        // TODO: Leios. A next block that certifies an endorser block then cannot say which:
        // the node must stop rather than apply it without the endorsed transactions.
        reqs.retain(|req| *req != intersection);
        manager.continue_sync(id);
    }

    type Reply = Vec<u8>;
    type Content = Vec<u8>;

    fn content(
        _manager: &mut NetworkManager,
        _peer: PeerId,
        _block: &SpecificPoint,
        body: Self::Reply,
    ) -> Option<Self::Content> {
        Some(body)
    }

    fn deliver(manager: &mut NetworkManager, peer: PeerId, block: SpecificPoint, body: Vec<u8>) {
        let at_intersection = manager.peers.get_mut(&peer).is_some_and(|data| {
            data.intersection.take_if(|intersection| *intersection == block).is_some()
        });
        if at_intersection {
            manager.handle_intersection_block(peer, &body);
        } else {
            manager.flow_handler.handle_block_fetched(block.slot, block.hash, body);
        }
    }
}

/// Requests asked of a peer and not served yet, each kind with its own stall check.
struct Requests {
    rbs: Pending,
}

impl Requests {
    fn new() -> Self {
        Self {
            rbs: Pending::new(),
        }
    }

    fn clear(&mut self) {
        self.rbs.reqs.clear();
    }

    fn len(&self) -> usize {
        self.rbs.reqs.len()
    }
}

/// Requests of one kind, with the state of their stall check.
struct Pending {
    reqs: Vec<SpecificPoint>,
    /// Whether a reply arrived since the last stall check.
    replied: bool,
    /// Whether requests were pending at the last stall check.
    awaited: bool,
}

impl Pending {
    fn new() -> Self {
        Self {
            reqs: vec![],
            replied: false,
            awaited: false,
        }
    }

    fn ack(&mut self, point: &SpecificPoint) {
        self.reqs.retain(|req| req != point);
    }

    /// Whether the peer sent no reply over a whole interval while asked.
    fn stalled(&self) -> bool {
        self.awaited && !self.replied && !self.reqs.is_empty()
    }

    /// Start the next interval.
    fn rearm(&mut self) {
        self.awaited = !self.reqs.is_empty();
        self.replied = false;
    }
}

struct PeerData {
    conn: PeerConnection,
    reqs: Requests,
    /// True once any protocol event has been received from this peer (ChainSync, BlockFetch,
    /// etc.). Used to distinguish a cold-promoted peer that never managed to connect from
    /// one that ran successfully and then disconnected.
    established: bool,
    /// Leios: the block at the intersection, until it arrives. A block may certify the
    /// endorser block its parent announced, and only the parent's header says which. The
    /// intersection is a point we gave, known by slot and hash only, and the block there is
    /// the parent of the first block chainsync brings: its header has to come first, so this
    /// peer is not asked for its next header meanwhile.
    intersection: Option<SpecificPoint>,
}

impl PeerData {
    fn new(conn: PeerConnection) -> Self {
        Self {
            conn,
            reqs: Requests::new(),
            established: false,
            intersection: None,
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

    /// Ask for the next header, unless the block at the intersection is still to arrive.
    fn continue_sync(&self) {
        if self.intersection.is_some() {
            return;
        }
        if let Err(error) = self.conn.continue_sync() {
            warn!(
                "could not continue sync with {}: {error:#}",
                self.conn.address
            );
        }
    }

    fn request<F: Fetch>(&mut self, point: &SpecificPoint) -> bool {
        if F::requests(self).reqs.contains(point) {
            return true;
        }
        if let Err(error) = F::send(&self.conn, point) {
            warn!(
                "could not request {:?} from {}: {error:#}",
                point, self.conn.address
            );
            return false;
        }
        F::requests_mut(self).reqs.push(point.clone());
        true
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
    /// Whether chainsync runs: peers are asked for their next header. Stops once requested
    /// blocks still to arrive reach [`MAX_PENDING_FETCHES`], runs again once they are down to
    /// [`RESUME_PENDING_FETCHES`]; in between, only this says which of the two applies.
    chainsync_running: bool,
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
    ipv6_enabled: bool,
    allow_non_public_peer_addrs: bool,
    discovery_interval: Duration,
    /// Bounds a new peer's TCP connect and its handshake (`connect-timeout-secs`).
    connect_timeout: Duration,
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
            chainsync_running: true,
            sync_point: None,
            flow_handler,
            peer_manager,
            min_hot_peers,
            cold_origin: HashSet::new(),
            configured_addrs,
            ipv6_enabled,
            allow_non_public_peer_addrs,
            discovery_interval: Duration::from_secs(discovery_interval_secs),
            connect_timeout: Duration::from_secs(connect_timeout_secs),
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
        let mut fetch_stall_ticker = time::interval(FETCH_STALL_INTERVAL);
        fetch_stall_ticker.tick().await; // skip the immediate first tick

        loop {
            tokio::select! {
                event = self.events.recv() => {
                    match event {
                        Some(e) => self.on_network_event(e).await?,
                        None => break,
                    }
                }
                _ = fetch_stall_ticker.tick() => {
                    self.on_fetch_stall_tick();
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
                self.update_chainsync();

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
                    self.request::<RBs>(&SpecificPoint { slot, hash }, announcers);
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
                // Requests no other peer announced went away with the dropped peers.
                self.update_chainsync();
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

        self.rerequest_inflight(victim.reqs.rbs.reqs);
        // Requests no other peer announced went away with the victim.
        self.update_chainsync();

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
        let reporter = PeerEventReporter::new(id, self.events_sender.clone());
        let conn = PeerConnection::new(
            address,
            self.network_magic,
            reporter,
            delay,
            self.connect_timeout,
        );
        let peer = PeerData::new(conn);
        let points = self.flow_handler.handle_new_connection(id, self.sync_point.as_ref());
        peer.find_intersect(points);
        self.peers.insert(id, peer);
        id
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
            PeerEvent::ChainSync(PeerChainSyncEvent::IntersectionFound(point)) => {
                self.fetch_intersection(peer, point);
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::RollForward(header, tip)) => {
                self.flow_handler.handle_tip(peer, tip);
                let slot = header.slot;
                let hash = header.hash;

                // TODO: Temporary. In Direct mode returns announcers then fetches blocks, in
                // Consensus mode: returns None, so no block fetch
                if let Some(peers) = self.flow_handler.handle_roll_forward(peer, header) {
                    self.request::<RBs>(&SpecificPoint { slot, hash }, peers);
                }
                // Only once this header's block is requested: it then counts toward
                // `MAX_PENDING_FETCHES` when deciding whether to ask for the next header.
                self.continue_sync(peer);
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::IntersectNotFound(tip)) => {
                info!(
                    "peer {:?} has no intersection with our chain, tip {:?}",
                    peer, tip
                );
                self.flow_handler.handle_tip(peer, tip);
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::EpochBoundary) => {
                self.continue_sync(peer);
            }
            PeerEvent::ChainSync(PeerChainSyncEvent::RollBackward(point, tip)) => {
                self.flow_handler.handle_tip(peer, tip);
                info!("peer {:?} rolled back to {:?}", peer, point);
                self.flow_handler.handle_roll_backward(peer, point);
                self.continue_sync(peer);
            }
            PeerEvent::BlockFetched(fetched) => self.handle_reply::<RBs>(
                peer,
                SpecificPoint {
                    slot: fetched.slot,
                    hash: fetched.hash,
                },
                fetched.body,
            ),
            PeerEvent::Disconnected => {
                self.handle_disconnect(peer);
            }
        }

        // Retry wants that were temporarily unresolved (e.g. out-of-order
        // BlockWanted vs announcement, or announcer churn during disconnects).
        self.retry_pending_wanted();
        // Bodies arrived or requests went away: chainsync may resume.
        self.update_chainsync();
    }

    /// Fetch the block at the intersection, and only then ask the peer for its next header
    /// (see [`PeerData::intersection`]).
    fn fetch_intersection(&mut self, id: PeerId, point: Point) {
        let Point::Specific(slot, hash) = point else {
            // Nothing precedes the first block.
            self.continue_sync(id);
            return;
        };
        let Ok(hash) = BlockHash::try_from(hash) else {
            warn!(
                ?id,
                slot, "intersection with a malformed hash; going on without its header"
            );
            self.continue_sync(id);
            return;
        };
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        };
        let block = SpecificPoint { slot, hash };
        if peer.request::<RBs>(&block) {
            peer.intersection = Some(block);
        } else {
            self.handle_disconnect(id);
        }
    }

    /// The block at the intersection arrived: only its header is needed, the block itself is
    /// already published.
    fn handle_intersection_block(&mut self, peer: PeerId, body: &[u8]) {
        match header_of(body) {
            // TODO: Leios. Keep it for the block after the intersection, which may certify
            // the endorser block it announced.
            Ok(Some(header)) => {
                debug!(?peer, hash = %header.hash, slot = header.slot, "intersection header")
            }
            // An epoch boundary block, which announces no endorser block.
            Ok(None) => {}
            Err(error) => warn!(?peer, "undecodable intersection header: {error:#}"),
        }
        self.continue_sync(peer);
    }

    /// Blocks requested from all peers whose body has not arrived yet.
    fn pending_fetches(&self) -> usize {
        self.peers.values().map(|peer| peer.reqs.len()).sum()
    }

    /// Ask the peer, which just sent a chainsync event, for its next header while chainsync
    /// runs.
    fn continue_sync(&mut self, id: PeerId) {
        if self.update_chainsync() {
            // Running again: every peer was just asked, this one included.
            return;
        }
        if self.chainsync_running
            && let Some(peer) = self.peers.get(&id)
        {
            peer.continue_sync();
        }
    }

    /// Stop chainsync once requested blocks reach [`MAX_PENDING_FETCHES`]; run it again once
    /// they are down to [`RESUME_PENDING_FETCHES`], asking every peer for its next header, and
    /// say so. A peer still waiting for a header ignores that: pallas-network2 asks only a peer
    /// whose chainsync is idle. Since txpipe/pallas#818 (in Sundae's `leios-musashi`), a peer
    /// whose `RequestNext` is queued but not sent yet still counts as idle: pallas defers a
    /// second `RequestNext` until the answer, so that peer sends one header more.
    fn update_chainsync(&mut self) -> bool {
        let pending = self.pending_fetches();
        if self.chainsync_running && pending >= MAX_PENDING_FETCHES {
            self.chainsync_running = false;
        } else if !self.chainsync_running && pending <= RESUME_PENDING_FETCHES {
            self.chainsync_running = true;
            for peer in self.peers.values() {
                peer.continue_sync();
            }
            return true;
        }
        false
    }

    /// Requests a peer leaves unanswered (e.g. a block it no longer has) would count forever
    /// and, once at the cap, hold every peer's chainsync.
    fn on_fetch_stall_tick(&mut self) {
        self.on_fetch_stall::<RBs>();
    }

    /// A peer that sent no reply over a whole interval while asked is asked for none of those
    /// requests any more: each goes to the peers [`Fetch::rerequest_to`] says, the stalled
    /// one's reply still used if it comes late.
    fn on_fetch_stall<F: Fetch>(&mut self) {
        let stalled: Vec<PeerId> = self
            .peers
            .iter()
            .filter(|(_, peer)| F::requests(peer).stalled())
            .map(|(id, _)| *id)
            .collect();
        for id in stalled {
            let Some(peer) = self.peers.get_mut(&id) else {
                continue;
            };
            let mut reqs = std::mem::take(&mut F::requests_mut(peer).reqs);
            warn!(
                address = %peer.conn.address,
                pending = reqs.len(),
                intersection = ?peer.intersection,
                "no {} over a stall interval; asking other peers",
                F::WHAT
            );
            F::on_stalled(self, id, &mut reqs);
            self.rerequest_elsewhere::<F>(id, reqs);
        }
        for peer in self.peers.values_mut() {
            F::requests_mut(peer).rearm();
        }
        self.update_chainsync();
    }

    fn rerequest_elsewhere<F: Fetch>(&mut self, stalled: PeerId, reqs: Vec<SpecificPoint>) {
        for req in reqs {
            let Some(others) = F::rerequest_to(&self.flow_handler, &req, stalled) else {
                continue; // no longer wanted
            };
            if others.is_empty() {
                warn!(
                    ?req,
                    "no peer announces it any more; no longer waiting for it"
                );
                continue;
            }
            self.request::<F>(&req, others);
        }
    }

    fn handle_reply<F: Fetch>(&mut self, peer: PeerId, point: SpecificPoint, reply: F::Reply) {
        self.mark_replied::<F>(peer);
        if let Some(content) = F::content(self, peer, &point, reply) {
            self.ack_on_every_peer::<F>(&point);
            F::deliver(self, peer, point, content);
        }
    }

    fn mark_replied<F: Fetch>(&mut self, peer: PeerId) {
        if let Some(data) = self.peers.get_mut(&peer) {
            F::requests_mut(data).replied = true;
        }
    }

    /// A reply serves the request wherever it was asked: one a stalled peer answers late was
    /// asked again of another peer meanwhile.
    fn ack_on_every_peer<F: Fetch>(&mut self, point: &SpecificPoint) {
        for peer in self.peers.values_mut() {
            F::requests_mut(peer).ack(point);
        }
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

        if self.configured_addrs.contains(&peer.conn.address) {
            warn!(address = %peer.conn.address, "disconnected from pre-configured peer");
        } else {
            info!(address = %peer.conn.address, "peer disconnected");
        }

        // Capture state before peer is partially consumed.
        let is_cold_origin = self.cold_origin.remove(&id);
        let is_configured = self.configured_addrs.contains(&peer.conn.address);
        let established = peer.established;
        let address = peer.conn.address.clone();

        // The next peer is temporary needed for Direct mode flow handler only
        self.flow_handler.handle_disconnect(id, self.peers.keys().next().copied());

        self.rerequest_inflight(peer.reqs.rbs.reqs);

        if self.peer_manager.is_none() {
            // Disabled mode: reconnect with 5s backoff
            self.handle_new_connection(address, Duration::from_secs(5));
            return;
        }

        // P2P mode.

        if is_cold_origin && !established && !is_configured {
            // Cold-promoted peer that never established a connection (TCP refused / timeout).
            // Blacklist so peer-sharing cannot re-add it this session.
            info!(
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
                self.request::<RBs>(&SpecificPoint { slot, hash }, announcers);
                self.pending_wanted.remove(&(slot, hash));
            }
        }
    }

    fn rerequest_inflight(&mut self, reqs: Vec<SpecificPoint>) {
        for block in reqs {
            if let Some(announcers) = self.flow_handler.block_announcers(block.slot, block.hash) {
                self.request::<RBs>(&block, announcers);
            }
        }
    }

    /// Ask the first of `announcers` that takes the request, unless a peer already has it.
    fn request<F: Fetch>(&mut self, point: &SpecificPoint, announcers: Vec<PeerId>) {
        for announcer in announcers {
            // Checked each time: the disconnect of a peer that failed to take it may have asked
            // another one.
            if self.is_asked::<F>(point) {
                return;
            }
            let Some(peer) = self.peers.get_mut(&announcer) else {
                continue;
            };
            if peer.request::<F>(point) {
                break; // only fetch from one
            } else {
                self.handle_disconnect(announcer);
            }
        }
    }

    fn is_asked<F: Fetch>(&self, point: &SpecificPoint) -> bool {
        self.peers.values().any(|peer| F::requests(peer).reqs.contains(point))
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(pub u64);

/// Reports a peer connection's events to the network manager, tagged with the peer's id.
/// In-process only: nothing goes to the remote peer.
pub struct PeerEventReporter {
    id: PeerId,
    sink: mpsc::Sender<NetworkEvent>,
}

impl PeerEventReporter {
    pub(crate) fn new(id: PeerId, sink: mpsc::Sender<NetworkEvent>) -> Self {
        Self { id, sink }
    }

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
    use crate::connection::{BlockFetched, Header, PeerChainSyncEvent, PeerEvent};
    use acropolis_common::configuration::BlockFlowMode;
    use acropolis_common::messages::Message;
    use acropolis_common::{BlockHash, Era};
    use acropolis_test_utils::mainnet_genesis_values;
    use caryatid_sdk::Context;
    use caryatid_sdk::mock_bus::MockBus;
    use config::Config;
    use pallas_network2::behavior::initiator::InitiatorCommand;
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
            genesis_values: mainnet_genesis_values(),
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
        let block_wanted_subscription =
            Some(context.message_bus.subscribe(&cfg.block_wanted_topic).await.unwrap());
        let flow_handler = BlockFlowHandler::new(
            &cfg,
            BlockFlowMode::Consensus,
            acropolis_common::params::SECURITY_PARAMETER_K,
            context.clone(),
            events_sender.clone(),
            block_wanted_subscription,
        );
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
        let reporter = PeerEventReporter::new(peer, manager.events_sender.clone());
        // Delay prevents immediate network activity; tests drive state manually.
        let conn = PeerConnection::new(
            "test-peer:3001".to_string(),
            0,
            reporter,
            Duration::from_secs(3600),
            manager.connect_timeout,
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
        let reporter = PeerEventReporter::new(peer, manager.events_sender.clone());
        let conn = PeerConnection::new(
            address.to_string(),
            0,
            reporter,
            Duration::from_secs(3600),
            manager.connect_timeout,
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

        let reqs = &manager.peers.get(&peer).unwrap().reqs.rbs.reqs;
        assert!(
            reqs.contains(&SpecificPoint { slot, hash }),
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
        let reqs = &manager.peers.get(&peer).unwrap().reqs.rbs.reqs;
        assert!(
            reqs.contains(&SpecificPoint { slot, hash }),
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

    #[tokio::test]
    async fn a_wanted_block_its_lone_announcer_does_not_serve_is_asked_of_the_next_one() {
        let mut manager = test_consensus_manager().await;
        let (stalled, next) = (PeerId(3), PeerId(4));
        add_test_peer(&mut manager, stalled);
        add_test_peer(&mut manager, next);
        let slot = 300;
        let hash = BlockHash::new([5; 32]);
        let announce = |manager: &mut NetworkManager, peer| {
            manager.handle_peer_update(
                peer,
                PeerEvent::ChainSync(PeerChainSyncEvent::RollForward(
                    test_header(slot, 30, hash, BlockHash::new([6; 32])),
                    Point::Specific(slot, hash.to_vec()),
                )),
            )
        };
        let block = SpecificPoint { slot, hash };
        announce(&mut manager, stalled);
        manager.on_network_event(NetworkEvent::BlockWanted { hash, slot }).await.unwrap();
        manager.on_fetch_stall_tick();
        manager.on_fetch_stall_tick();
        assert!(
            manager.peers[&stalled].reqs.rbs.reqs.contains(&block),
            "block given up on while its lone announcer is still there"
        );

        announce(&mut manager, next);
        manager.on_fetch_stall_tick();

        assert!(
            manager.peers[&next].reqs.rbs.reqs.contains(&block),
            "wanted block not asked of the peer announcing it next"
        );
    }

    #[tokio::test]
    async fn chainsync_pauses_at_the_fetch_cap_until_half_of_it() {
        let mut manager = test_consensus_manager().await;
        let peer = PeerId(4);
        let (conn, mut commands) = PeerConnection::detached("peer.test:3001");
        manager.peers.insert(peer, PeerData::new(conn));
        let mut continued = || {
            std::iter::from_fn(|| commands.try_recv().ok())
                .filter(|cmd| matches!(cmd, InitiatorCommand::ContinueSync(_)))
                .count()
        };
        // One hash per slot: a body acknowledges every request for its hash.
        let hash_at = |slot: u64| {
            let mut bytes = [0; 32];
            bytes[..8].copy_from_slice(&slot.to_le_bytes());
            BlockHash::new(bytes)
        };
        let fetched = |slot: u64| {
            PeerEvent::BlockFetched(BlockFetched {
                slot,
                hash: hash_at(slot),
                body: vec![],
            })
        };
        manager.peers.get_mut(&peer).unwrap().reqs.rbs.reqs = (0..MAX_PENDING_FETCHES as u64)
            .map(|slot| SpecificPoint {
                slot,
                hash: hash_at(slot),
            })
            .collect();

        manager.handle_peer_update(
            peer,
            PeerEvent::ChainSync(PeerChainSyncEvent::IntersectionFound(Point::Origin)),
        );
        assert_eq!(continued(), 0, "next header asked at the cap");

        // Below the cap, above half of it: still paused, also for a peer reporting a header.
        let to_resume = (MAX_PENDING_FETCHES - RESUME_PENDING_FETCHES) as u64;
        for slot in 0..to_resume - 1 {
            manager.handle_peer_update(peer, fetched(slot));
        }
        manager.handle_peer_update(
            peer,
            PeerEvent::ChainSync(PeerChainSyncEvent::EpochBoundary),
        );
        assert_eq!(continued(), 0, "next header asked while paused");

        manager.handle_peer_update(peer, fetched(to_resume - 1));
        assert_eq!(continued(), 1, "next header not asked at half the cap");
    }

    #[tokio::test]
    async fn blocks_a_peer_does_not_serve_are_asked_of_another_announcer() {
        let mut manager = test_consensus_manager().await;
        let (stalled, other) = (PeerId(5), PeerId(6));
        add_test_peer(&mut manager, stalled);
        add_test_peer(&mut manager, other);
        let slot = 400;
        let hash = BlockHash::new([7; 32]);
        for peer in [stalled, other] {
            manager.flow_handler.handle_tip(peer, Point::Specific(slot, hash.to_vec()));
            let _ = manager
                .flow_handler
                .handle_roll_forward(peer, test_header(slot, 40, hash, BlockHash::new([8; 32])));
        }
        manager.request::<RBs>(&SpecificPoint { slot, hash }, vec![stalled]);

        // The first check sees the request, the second one a whole interval without a body.
        manager.on_fetch_stall_tick();
        assert!(manager.peers[&stalled].reqs.rbs.reqs.contains(&SpecificPoint { slot, hash }));
        manager.on_fetch_stall_tick();

        assert!(
            manager.peers[&stalled].reqs.rbs.reqs.is_empty(),
            "request left on the stalled peer"
        );
        assert!(
            manager.peers[&other].reqs.rbs.reqs.contains(&SpecificPoint { slot, hash }),
            "block not asked of the other announcer"
        );

        // Asked for again while the other peer has it in flight.
        manager.request::<RBs>(&SpecificPoint { slot, hash }, vec![stalled, other]);
        assert!(
            manager.peers[&stalled].reqs.rbs.reqs.is_empty(),
            "block asked of two peers at once"
        );
    }

    /// A Conway block from preview, already in the repository for the bootstrapper tests.
    const BLOCK: &[u8] = include_bytes!(
        "../../snapshot_bootstrapper/data/preview/blocks/block.86399953.5d37babd87c35b9d5464b8e6c81f655a2fae44a422a1104ca67a7f72476286af.cbor"
    );
    const BLOCK_SLOT: u64 = 86399953;

    fn block_hash() -> BlockHash {
        BlockHash::new(*pallas::ledger::traverse::MultiEraBlock::decode(BLOCK).unwrap().hash())
    }

    /// A peer found at a specific intersection, with the commands sent to it.
    async fn peer_at_block_intersection() -> (
        NetworkManager,
        PeerId,
        mpsc::UnboundedReceiver<InitiatorCommand>,
    ) {
        let mut manager = test_consensus_manager().await;
        let peer = PeerId(7);
        let (conn, commands) = PeerConnection::detached("peer.test:3001");
        manager.peers.insert(peer, PeerData::new(conn));
        manager.handle_peer_update(
            peer,
            PeerEvent::ChainSync(PeerChainSyncEvent::IntersectionFound(Point::Specific(
                BLOCK_SLOT,
                block_hash().to_vec(),
            ))),
        );
        (manager, peer, commands)
    }

    fn sent(commands: &mut mpsc::UnboundedReceiver<InitiatorCommand>) -> Vec<InitiatorCommand> {
        std::iter::from_fn(|| commands.try_recv().ok()).collect()
    }

    /// Leios: the next block may certify the endorser block announced at the intersection, and
    /// only the header of the block there says which: chainsync waits for that block.
    #[tokio::test]
    async fn chainsync_waits_for_the_block_at_the_intersection() {
        let (mut manager, peer, mut commands) = peer_at_block_intersection().await;
        let asked = sent(&mut commands);
        assert!(
            matches!(&asked[..], [InitiatorCommand::RequestBlocks(_)]),
            "the block at the intersection only, got {asked:?}"
        );

        manager.handle_peer_update(
            peer,
            PeerEvent::BlockFetched(BlockFetched {
                slot: BLOCK_SLOT,
                hash: block_hash(),
                body: BLOCK.to_vec(),
            }),
        );
        let asked = sent(&mut commands);
        assert!(
            matches!(&asked[..], [InitiatorCommand::ContinueSync(_)]),
            "next header once the block arrived, got {asked:?}"
        );
    }

    /// A peer that never serves the block at the intersection does not hold chainsync forever.
    #[tokio::test]
    async fn chainsync_goes_on_without_an_unserved_intersection_block() {
        let (mut manager, peer, mut commands) = peer_at_block_intersection().await;
        sent(&mut commands);

        // The first check sees the request, the second one a whole interval without a body.
        manager.on_fetch_stall_tick();
        manager.on_fetch_stall_tick();

        let asked = sent(&mut commands);
        assert!(
            matches!(&asked[..], [InitiatorCommand::ContinueSync(_)]),
            "next header after the stall, got {asked:?}"
        );
        assert!(
            manager.peers[&peer].reqs.rbs.reqs.is_empty(),
            "block still awaited"
        );
    }
}
