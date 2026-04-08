use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use acropolis_common::BlockHash;
use acropolis_common::messages::{
    BlockOfferedMessage, BlockRejectedMessage, BlockRescindedMessage, BlockWantedMessage,
    CardanoMessage, ConsensusMessage, Message,
};
use acropolis_common::params::SECURITY_PARAMETER_K;
use anyhow::Result;
use caryatid_sdk::{Context, Subscription};
use pallas::network::miniprotocols::Point;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::BlockSink;
use crate::chain_state::{ChainEvent, ChainState, SpecificPoint};
use crate::configuration::InterfaceConfig;
use crate::connection::Header;
use crate::network::{NetworkEvent, PeerId};
use acropolis_common::configuration::BlockFlowMode;

/// Block flow handling strategies.
pub enum BlockFlowHandler {
    /// Direct: auto-fetch blocks as announced, PNI manages chain selection.
    /// Contains ChainState which tracks preferred upstream and publishes chain events.
    Direct { chain: ChainState },
    /// Consensus-driven: publish offers, wait for 'block wants' before fetching.
    /// Chain selection is delegated to the consensus module.
    /// Tracks block announcers.
    Consensus(ConsensusFlowState),
}

impl BlockFlowHandler {
    pub async fn new(
        config: &InterfaceConfig,
        block_flow_mode: BlockFlowMode,
        context: Arc<Context<Message>>,
        events_sender: mpsc::Sender<NetworkEvent>,
    ) -> Result<Self> {
        let security_param_k = Arc::new(AtomicU64::new(SECURITY_PARAMETER_K));

        let params_subscription = context.subscribe(&config.protocol_params_topic).await?;
        context.run(Self::watch_protocol_params(
            params_subscription,
            Arc::clone(&security_param_k),
        ));

        match block_flow_mode {
            BlockFlowMode::Direct => {
                info!("Block flow mode: Direct (auto-fetch)");
                Ok(BlockFlowHandler::Direct {
                    chain: ChainState::new(Arc::clone(&security_param_k)),
                })
            }
            BlockFlowMode::Consensus => {
                info!(
                    "Block flow mode: Consensus (offers on '{}', wants on '{}')",
                    config.consensus_topic, config.block_wanted_topic
                );
                let subscription = context.subscribe(&config.block_wanted_topic).await?;
                context.run(Self::forward_block_wanted_to_events(
                    subscription,
                    events_sender,
                ));
                Ok(BlockFlowHandler::Consensus(ConsensusFlowState::new(
                    context,
                    config.consensus_topic.clone(),
                    security_param_k,
                )))
            }
        }
    }

    /// Set the initial security parameter from genesis values.
    /// Should be called once after genesis values are available, before sync starts.
    pub fn set_genesis_security_param(&self, k: u64) {
        match self {
            BlockFlowHandler::Direct { chain } => {
                chain.security_param_k.store(k, Ordering::Release)
            }
            BlockFlowHandler::Consensus(state) => {
                state.security_param_k.store(k, Ordering::Release)
            }
        }
    }

    async fn watch_protocol_params(
        mut subscription: Box<dyn Subscription<Message>>,
        security_param_k: Arc<AtomicU64>,
    ) {
        while let Ok((_, msg)) = subscription.read().await {
            if let Message::Cardano((_, CardanoMessage::ProtocolParams(params))) = msg.as_ref() {
                let new_k =
                    params.params.shelley.as_ref().map(|s| s.security_param as u64).or_else(|| {
                        params.params.byron.as_ref().map(|b| b.protocol_consts.k as u64)
                    });
                if let Some(new_k) = new_k {
                    security_param_k.store(new_k, Ordering::Release);
                }
            }
        }
        error!("protocol params subscription closed");
    }

    async fn forward_block_wanted_to_events(
        mut subscription: Box<dyn Subscription<Message>>,
        events_sender: mpsc::Sender<NetworkEvent>,
    ) {
        while let Ok((_, msg)) = subscription.read().await {
            let event = match msg.as_ref() {
                Message::Consensus(ConsensusMessage::BlockWanted(BlockWantedMessage {
                    hash,
                    slot,
                })) => Some(NetworkEvent::BlockWanted {
                    hash: *hash,
                    slot: *slot,
                }),
                Message::Consensus(ConsensusMessage::BlockRejected(BlockRejectedMessage {
                    hash,
                    slot,
                })) => Some(NetworkEvent::BlockRejected {
                    hash: *hash,
                    slot: *slot,
                }),
                _ => None,
            };
            if let Some(event) = event
                && events_sender.send(event).await.is_err()
            {
                error!("event channel closed");
                return;
            }
        }
        error!("subscription closed");
    }

    /// Handle a peer announcing a block (roll forward).
    /// Returns peers to fetch immediately (Direct), or None in case of consensus mode.
    pub fn handle_roll_forward(&mut self, peer: PeerId, header: Header) -> Option<Vec<PeerId>> {
        match self {
            BlockFlowHandler::Direct { chain } => {
                let announcers = chain.handle_roll_forward(peer, header);
                if announcers.is_empty() {
                    None
                } else {
                    Some(announcers)
                }
            }
            BlockFlowHandler::Consensus(state) => {
                state.handle_roll_forward(peer, &header);
                None
            }
        }
    }

    /// Handle a peer rolling back.
    pub fn handle_roll_backward(&mut self, peer: PeerId, point: Point) {
        match self {
            BlockFlowHandler::Direct { chain } => {
                chain.handle_roll_backward(peer, point);
            }
            BlockFlowHandler::Consensus(state) => {
                state.handle_roll_backward(peer, point);
            }
        }
    }

    /// Handle a peer reporting its tip.
    pub fn handle_tip(&mut self, peer: PeerId, tip: Point) {
        match self {
            BlockFlowHandler::Direct { chain } => chain.handle_tip(peer, tip),
            BlockFlowHandler::Consensus(state) => state.handle_tip(peer, tip),
        }
    }

    /// Handle a block body being fetched.
    pub fn handle_block_fetched(&mut self, slot: u64, hash: BlockHash, body: Vec<u8>) {
        match self {
            BlockFlowHandler::Direct { chain } => chain.handle_body_fetched(slot, hash, body),
            BlockFlowHandler::Consensus(state) => state.handle_block_fetched(slot, hash, body),
        }
    }

    /// Handle a peer disconnecting.
    pub fn handle_disconnect(&mut self, peer: PeerId, next_peer: Option<PeerId>) {
        match self {
            BlockFlowHandler::Direct { chain } => chain.handle_disconnect(peer, next_peer),
            BlockFlowHandler::Consensus(state) => state.handle_disconnect(peer),
        }
    }

    /// Set the preferred upstream peer (Direct mode only, Consensus no-op).
    pub fn set_preferred_upstream(&mut self, peer: Option<PeerId>) {
        if let BlockFlowHandler::Direct { chain } = self {
            if let Some(peer_id) = peer {
                chain.handle_new_preferred_upstream(peer_id);
            } else {
                warn!("Sync requested but no upstream peers available");
            }
        }
    }

    /// Get the preferred upstream peer (Direct mode), or None (Consensus mode).
    pub fn preferred_upstream(&self) -> Option<PeerId> {
        match self {
            BlockFlowHandler::Direct { chain } => chain.preferred_upstream,
            BlockFlowHandler::Consensus(_) => None,
        }
    }

    /// Handle a new peer connection.
    /// Returns the points to use for find_intersect, if any.
    /// Uses chain state points (or falls back to sync_point).
    pub fn handle_new_connection(
        &mut self,
        peer: PeerId,
        sync_point: Option<&Point>,
    ) -> Vec<Point> {
        match self {
            BlockFlowHandler::Direct { chain } => {
                if chain.preferred_upstream.is_none() {
                    chain.handle_new_preferred_upstream(peer);
                }
                let points = chain.choose_points_for_find_intersect();
                if !points.is_empty() {
                    return points;
                }
            }
            BlockFlowHandler::Consensus(state) => {
                let points = state.choose_points_for_find_intersect();
                if !points.is_empty() {
                    return points;
                }
            }
        }
        sync_point.map(|p| vec![p.clone()]).unwrap_or_default()
    }

    /// Get peers that announced a specific block.
    pub fn block_announcers(&self, slot: u64, hash: BlockHash) -> Option<Vec<PeerId>> {
        let announcers = match self {
            BlockFlowHandler::Direct { chain } => chain.block_announcers(slot, hash),
            BlockFlowHandler::Consensus(state) => state.block_announcers(slot, hash),
        };

        (!announcers.is_empty()).then_some(announcers)
    }

    /// Whether the flow has ever seen this exact block identity (slot+hash).
    pub fn knows_block(&self, slot: u64, hash: BlockHash) -> bool {
        match self {
            BlockFlowHandler::Direct { chain } => !chain.block_announcers(slot, hash).is_empty(),
            BlockFlowHandler::Consensus(state) => state.knows_block(slot, hash),
        }
    }

    /// Reset state for a new sync point.
    pub fn handle_sync_reset(&mut self) {
        match self {
            BlockFlowHandler::Direct { chain } => {
                let k = Arc::clone(&chain.security_param_k);
                *chain = ChainState::new(k);
            }
            BlockFlowHandler::Consensus(state) => state.handle_sync_reset(),
        }
    }

    /// Return the peers that announced a rejected block, consuming the entry.
    pub fn block_rejected_announcers(&mut self, hash: BlockHash) -> Vec<PeerId> {
        match self {
            BlockFlowHandler::Direct { .. } => Vec::new(),
            BlockFlowHandler::Consensus(state) => state.block_rejected_announcers(hash),
        }
    }

    /// Publish events appropriate for the current flow mode.
    ///
    /// - Direct mode: publishes RollForward/RollBackward from chain state
    /// - Consensus mode: publishes BlockOffered/BlockRescinded/BlockAvailable to consensus
    pub async fn publish(
        &mut self,
        block_sink: &mut BlockSink,
        published_blocks: &mut u64,
    ) -> Result<()> {
        match self {
            BlockFlowHandler::Direct { chain } => {
                while let Some(event) = chain.next_unpublished_event() {
                    let tip = chain.preferred_upstream_tip();
                    match event {
                        ChainEvent::RollForward { header, body } => {
                            block_sink.announce_roll_forward(header, body, tip).await?;
                            *published_blocks += 1;
                            if published_blocks.is_multiple_of(100) {
                                info!("Published block {}", header.number);
                            }
                        }
                        ChainEvent::RollBackward { header } => {
                            block_sink.announce_roll_backward(header, tip).await?;
                        }
                    }
                    chain.handle_event_published();
                }
            }
            BlockFlowHandler::Consensus(state) => {
                state.publish_pending(block_sink, published_blocks).await?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConsensusBlockEvent {
    Offered {
        hash: BlockHash,
        slot: u64,
        number: u64,
        parent_hash: BlockHash,
    },
    Fetched {
        header: Header,
        body: Vec<u8>,
    },
    Rescinded {
        hash: BlockHash,
        slot: u64,
    },
}

/// Tracks block announcements from peers and generates consensus events.
///
/// - Tracks explicit announcements and rollbacks
/// - Tracks peers' tips to filter out peers that can no longer serve a block
#[derive(Default)]
struct BlockTracker {
    /// (slot, hash) -> list of peers that announced it.
    /// Ordered by slot for efficient rollbacks.
    blocks: BTreeMap<(u64, BlockHash), Vec<PeerId>>,
    /// Each peer's last reported chain tip.
    tips: HashMap<PeerId, Point>,
    /// Pending consensus events to publish.
    pending_events: Vec<ConsensusBlockEvent>,
    /// Peers retained after a block is fetched, until consensus sends
    /// `BlockRejected`.
    /// Also used as a fallback source for `BlockWanted` re-requests:
    /// a block can become wanted again after it has already been fetched.
    fetched: HashMap<BlockHash, FetchedAnnouncers>,
}

#[derive(Debug, Clone)]
struct FetchedAnnouncers {
    slot: u64,
    peers: Vec<PeerId>,
}

impl BlockTracker {
    fn new() -> Self {
        Self::default()
    }

    /// Record a block announcement from a peer.
    /// Generates a ConsensusBlockEvent::Offered event if this is the first announcement for this block.
    fn track_announcement(
        &mut self,
        peer: PeerId,
        slot: u64,
        number: u64,
        hash: BlockHash,
        parent_hash: BlockHash,
    ) {
        // Already fetched — just record the peer for sanctioning, no re-offer.
        if let Some(entry) = self.fetched.get_mut(&hash) {
            if !entry.peers.contains(&peer) {
                entry.peers.push(peer);
            }
            return;
        }

        let is_new = !self.blocks.contains_key(&(slot, hash));
        let announcers = self.blocks.entry((slot, hash)).or_default();
        if !announcers.contains(&peer) {
            announcers.push(peer);
        }
        if is_new {
            self.pending_events.push(ConsensusBlockEvent::Offered {
                hash,
                slot,
                number,
                parent_hash,
            });
        }
    }

    /// Update a peer's known tip.
    fn handle_tip(&mut self, peer: PeerId, tip: Point) {
        self.tips.insert(peer, tip);
    }

    /// Handle a peer rollback.
    ///
    /// For `Point::Specific(slot, hash)`, the peer is considered to be on exactly
    /// that block after rollback, so we remove it from:
    /// - all blocks above `slot`
    /// - sibling hashes at the same `slot`
    /// - and keep it only on `(slot, hash)`.
    fn handle_rollback(&mut self, peer: PeerId, point: &Point) {
        match point {
            Point::Origin => {
                for announcers in self.blocks.values_mut() {
                    announcers.retain(|p| *p != peer);
                }
                for fetched in self.fetched.values_mut() {
                    fetched.peers.retain(|p| *p != peer);
                }
            }
            Point::Specific(rollback_to_slot, rollback_to_hash) => {
                let rollback_keep = BlockHash::try_from(rollback_to_hash.as_slice())
                    .ok()
                    .map(|h| (*rollback_to_slot, h));
                for ((slot, hash), announcers) in &mut self.blocks {
                    if *slot >= *rollback_to_slot && rollback_keep != Some((*slot, *hash)) {
                        announcers.retain(|p| *p != peer);
                    }
                }
                for (hash, fetched) in &mut self.fetched {
                    if fetched.slot >= *rollback_to_slot
                        && rollback_keep != Some((fetched.slot, *hash))
                    {
                        fetched.peers.retain(|p| *p != peer);
                    }
                }
            }
        }
        self.rescind_orphaned_blocks();
        self.fetched.retain(|_, fetched| !fetched.peers.is_empty());
    }

    /// Handle a peer disconnecting — remove it from all blocks and tips.
    /// Blocks whose announcer list drops to zero are rescinded: a
    /// `Rescinded` event is pushed to `pending_events` for each.
    fn handle_disconnect(&mut self, peer: PeerId) {
        self.tips.remove(&peer);
        for announcers in self.blocks.values_mut() {
            announcers.retain(|p| *p != peer);
        }
        for fetched in self.fetched.values_mut() {
            fetched.peers.retain(|p| *p != peer);
        }
        self.rescind_orphaned_blocks();
        self.fetched.retain(|_, fetched| !fetched.peers.is_empty());
    }

    /// Remove blocks with no remaining announcers and emit Rescinded events.
    fn rescind_orphaned_blocks(&mut self) {
        let mut rescinded = Vec::new();
        self.blocks.retain(|key, announcers| {
            if announcers.is_empty() {
                rescinded.push(*key);
                false
            } else {
                true
            }
        });
        for (slot, hash) in rescinded {
            self.pending_events.push(ConsensusBlockEvent::Rescinded { hash, slot });
        }
    }

    /// Checks if a peer's tip is at or beyond the given slot.
    fn peer_can_have_block(&self, peer: PeerId, slot: u64) -> bool {
        match self.tips.get(&peer) {
            Some(Point::Specific(peer_slot, _)) => *peer_slot >= slot,
            Some(Point::Origin) | None => false,
        }
    }

    fn candidate_announcers(&self, slot: u64, hash: BlockHash) -> Vec<PeerId> {
        if let Some(peers) = self.blocks.get(&(slot, hash)) {
            return peers.clone();
        }
        self.fetched
            .get(&hash)
            .filter(|fetched| fetched.slot == slot)
            .map(|fetched| fetched.peers.clone())
            .unwrap_or_default()
    }

    fn knows_block(&self, slot: u64, hash: BlockHash) -> bool {
        !self.candidate_announcers(slot, hash).is_empty()
    }

    /// Get peers that announced a block and whose tip still covers it.
    fn announcers(&self, slot: u64, hash: BlockHash) -> Vec<PeerId> {
        let candidates = self.candidate_announcers(slot, hash);
        if candidates.is_empty() {
            return candidates;
        }

        let filtered: Vec<PeerId> = candidates
            .iter()
            .copied()
            .filter(|peer| self.peer_can_have_block(*peer, slot))
            .collect();

        // If all tips look stale, still return known announcers as a best-effort
        // fallback. Tip signals can lag behind announcements.
        if filtered.is_empty() {
            candidates
        } else {
            filtered
        }
    }

    /// A block was successfully fetched — move its announcing peers from
    /// `blocks` to `fetched` so they can be sanctioned if consensus later
    /// rejects the block.
    fn block_fetched(&mut self, slot: u64, hash: BlockHash) {
        let peers = self.blocks.remove(&(slot, hash)).unwrap_or_default();
        if !peers.is_empty() {
            self.fetched.insert(hash, FetchedAnnouncers { slot, peers });
        }
    }

    /// Consume and return the peers that announced a fetched block, to be
    /// sanctioned after a `BlockRejected` from consensus.
    fn take_rejected_announcers(&mut self, hash: BlockHash) -> Vec<PeerId> {
        self.fetched.remove(&hash).map(|f| f.peers).unwrap_or_default()
    }

    /// Take all pending events for publishing.
    fn take_events(&mut self) -> Vec<ConsensusBlockEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Clear all state.
    fn reset(&mut self) {
        self.blocks.clear();
        self.tips.clear();
        self.pending_events.clear();
        self.fetched.clear();
    }
}

pub struct ConsensusFlowState {
    context: Arc<Context<Message>>,
    topic: String,
    tracker: BlockTracker,
    blocks_offered_count: u64,
    blocks_published_count: u64,
    headers: HashMap<(u64, BlockHash), Header>,
    published_points: VecDeque<SpecificPoint>,
    security_param_k: Arc<AtomicU64>,
}

impl ConsensusFlowState {
    fn new(
        context: Arc<Context<Message>>,
        topic: String,
        security_param_k: Arc<AtomicU64>,
    ) -> Self {
        Self {
            context,
            topic,
            tracker: BlockTracker::new(),
            blocks_offered_count: 0,
            blocks_published_count: 0,
            headers: HashMap::new(),
            published_points: VecDeque::new(),
            security_param_k,
        }
    }

    fn choose_points_for_find_intersect(&self) -> Vec<Point> {
        SpecificPoint::choose_intersect_points(&self.published_points)
    }

    fn handle_roll_forward(&mut self, peer: PeerId, header: &Header) {
        let parent_hash = header.parent_hash.unwrap_or_default();
        self.headers.entry((header.slot, header.hash)).or_insert_with(|| header.clone());
        self.tracker.track_announcement(peer, header.slot, header.number, header.hash, parent_hash);
    }

    fn handle_roll_backward(&mut self, peer: PeerId, point: Point) {
        self.tracker.handle_rollback(peer, &point);

        let rollback_slot = match &point {
            Point::Specific(slot, _) => *slot,
            Point::Origin => 0,
        };
        while self.published_points.back().is_some_and(|p| p.slot > rollback_slot) {
            self.published_points.pop_back();
        }
    }

    fn handle_tip(&mut self, peer: PeerId, tip: Point) {
        self.tracker.handle_tip(peer, tip);
    }

    fn handle_disconnect(&mut self, peer: PeerId) {
        self.tracker.handle_disconnect(peer);
    }

    fn block_announcers(&self, slot: u64, hash: BlockHash) -> Vec<PeerId> {
        self.tracker.announcers(slot, hash)
    }

    fn knows_block(&self, slot: u64, hash: BlockHash) -> bool {
        self.tracker.knows_block(slot, hash)
    }

    /// Return the peers that announced a rejected block and clear the entry.
    ///
    /// Callers should disconnect each returned peer to sanction them for
    /// providing an invalid block.
    fn block_rejected_announcers(&mut self, hash: BlockHash) -> Vec<PeerId> {
        self.tracker.take_rejected_announcers(hash)
    }

    async fn publish_pending(
        &mut self,
        block_sink: &mut BlockSink,
        published_blocks: &mut u64,
    ) -> Result<()> {
        for event in self.tracker.take_events() {
            match event {
                ConsensusBlockEvent::Offered {
                    hash,
                    slot,
                    number,
                    parent_hash,
                } => {
                    self.blocks_offered_count += 1;
                    if self.blocks_offered_count.is_multiple_of(100) {
                        info!("Offered block (consensus) {}", hash);
                    }

                    let message = Arc::new(Message::Consensus(ConsensusMessage::BlockOffered(
                        BlockOfferedMessage {
                            hash,
                            slot,
                            number,
                            parent_hash,
                        },
                    )));
                    if let Err(e) = self.context.publish(&self.topic, message).await {
                        error!("Failed to publish consensus event: {e}");
                    }
                }
                ConsensusBlockEvent::Fetched { header, body } => {
                    block_sink.announce_roll_forward(&header, &body, None).await?;
                    self.published_points.push_back(SpecificPoint {
                        slot: header.slot,
                        hash: header.hash,
                    });
                    let k = self.security_param_k.load(Ordering::Acquire) as usize;
                    while self.published_points.len() > k {
                        self.published_points.pop_front();
                    }
                    *published_blocks += 1;
                    self.blocks_published_count += 1;
                    if self.blocks_published_count.is_multiple_of(100) {
                        info!("Published block {} (consensus)", header.number);
                    }
                }
                ConsensusBlockEvent::Rescinded { hash, slot } => {
                    let message = Arc::new(Message::Consensus(ConsensusMessage::BlockRescinded(
                        BlockRescindedMessage { hash, slot },
                    )));
                    if let Err(e) = self.context.publish(&self.topic, message).await {
                        error!("Failed to publish Rescinded for {hash}: {e}");
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_block_fetched(&mut self, slot: u64, hash: BlockHash, body: Vec<u8>) {
        self.tracker.block_fetched(slot, hash);
        if let Some(header) = self.headers.remove(&(slot, hash)) {
            self.tracker.pending_events.push(ConsensusBlockEvent::Fetched { header, body });
        } else {
            warn!("No stored header for fetched block {hash} at slot {slot}");
        }
    }

    fn handle_sync_reset(&mut self) {
        self.tracker.reset();
        self.headers.clear();
        self.published_points.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain_state::ChainState;
    use crate::network::PeerId;

    const PEER_1: PeerId = PeerId(1);
    const PEER_2: PeerId = PeerId(2);

    const GENESIS_HASH: BlockHash = BlockHash::new([0; 32]);
    const BLOCK_HASH_A: BlockHash = BlockHash::new([1; 32]);
    const BLOCK_HASH_B: BlockHash = BlockHash::new([2; 32]);
    const BLOCK_HASH_C: BlockHash = BlockHash::new([3; 32]);
    const BLOCK_HASH_D: BlockHash = BlockHash::new([4; 32]);
    const BLOCK_HASH_E: BlockHash = BlockHash::new([5; 32]);

    fn make_test_chain_state() -> ChainState {
        ChainState::new(Arc::new(AtomicU64::new(SECURITY_PARAMETER_K)))
    }

    fn point(slot: u64, hash: BlockHash) -> Point {
        Point::Specific(slot, hash.to_vec())
    }

    #[test]
    fn first_announcement_emits_offer() {
        let mut tracker = BlockTracker::new();

        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);

        assert!(tracker.blocks.contains_key(&(100, BLOCK_HASH_A)));
        let events = tracker.take_events();
        assert!(matches!(
            &events[..],
            [ConsensusBlockEvent::Offered { slot: 100, number: 1, hash, parent_hash }]
                if *hash == BLOCK_HASH_A && *parent_hash == GENESIS_HASH
        ));
    }

    #[test]
    fn duplicate_announcement_emits_single_offer() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));
        tracker.handle_tip(PEER_2, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, 1, BLOCK_HASH_A, GENESIS_HASH);

        assert_eq!(tracker.blocks.len(), 1);
        assert_eq!(tracker.announcers(100, BLOCK_HASH_A).len(), 2);
        assert_eq!(tracker.take_events().len(), 1);
    }

    #[test]
    fn fork_at_same_slot_tracks_both_blocks() {
        let mut tracker = BlockTracker::new();

        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, 1, BLOCK_HASH_B, GENESIS_HASH);

        assert_eq!(tracker.blocks.len(), 2);
        assert_eq!(tracker.take_events().len(), 2);
    }

    #[test]
    fn block_fetched_removes_from_tracking() {
        let mut tracker = BlockTracker::new();

        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_1, 101, 2, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.block_fetched(100, BLOCK_HASH_A);

        assert!(!tracker.blocks.contains_key(&(100, BLOCK_HASH_A)));
        assert!(tracker.blocks.contains_key(&(101, BLOCK_HASH_B)));
    }

    #[test]
    fn rollback_removes_peer_beyond_slot() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(200, BLOCK_HASH_B));
        tracker.handle_tip(PEER_2, point(200, BLOCK_HASH_B));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_1, 200, 2, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.track_announcement(PEER_2, 200, 2, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.handle_rollback(PEER_1, &point(100, BLOCK_HASH_A));
        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));

        assert_eq!(tracker.announcers(100, BLOCK_HASH_A), vec![PEER_1]);
        assert_eq!(tracker.announcers(200, BLOCK_HASH_B), vec![PEER_2]);
    }

    #[test]
    fn rollback_removes_peer_from_same_slot_sibling_hashes() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));
        tracker.handle_tip(PEER_2, point(100, BLOCK_HASH_B));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_B, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, 1, BLOCK_HASH_B, GENESIS_HASH);
        let _ = tracker.take_events();

        tracker.handle_rollback(PEER_1, &point(100, BLOCK_HASH_A));

        assert_eq!(tracker.announcers(100, BLOCK_HASH_A), vec![PEER_1]);
        assert_eq!(tracker.announcers(100, BLOCK_HASH_B), vec![PEER_2]);
    }

    #[test]
    fn disconnect_removes_peer_from_all_blocks() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(200, BLOCK_HASH_B));
        tracker.handle_tip(PEER_2, point(200, BLOCK_HASH_B));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_1, 200, 2, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.track_announcement(PEER_2, 200, 2, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.handle_disconnect(PEER_1);

        assert!(tracker.announcers(100, BLOCK_HASH_A).is_empty());
        assert_eq!(tracker.announcers(200, BLOCK_HASH_B), vec![PEER_2]);
    }

    #[test]
    fn peer_without_tip_excluded_from_announcers() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_2, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, 1, BLOCK_HASH_A, GENESIS_HASH);

        assert_eq!(tracker.announcers(100, BLOCK_HASH_A), vec![PEER_2]);
    }

    #[test]
    fn stale_tip_falls_back_to_known_announcer() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(200, BLOCK_HASH_B));
        tracker.track_announcement(PEER_1, 200, 2, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));

        assert_eq!(tracker.announcers(200, BLOCK_HASH_B), vec![PEER_1]);
    }

    #[test]
    fn reset_clears_all_state() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.block_fetched(100, BLOCK_HASH_A);
        tracker.reset();

        assert!(tracker.blocks.is_empty());
        assert!(tracker.tips.is_empty());
        assert!(tracker.fetched.is_empty());
        assert!(tracker.take_events().is_empty());
    }

    #[test]
    fn direct_mode_tracks_and_returns_announcers() {
        let mut handler = BlockFlowHandler::Direct {
            chain: make_test_chain_state(),
        };
        handler.set_preferred_upstream(Some(PEER_1));

        let header = Header {
            hash: BLOCK_HASH_A,
            slot: 100,
            number: 100,
            bytes: vec![],
            era: acropolis_common::Era::Conway,
            parent_hash: Some(GENESIS_HASH),
        };

        let result = handler.handle_roll_forward(PEER_1, header.clone());
        assert_eq!(result, Some(vec![PEER_1]));

        let result = handler.handle_roll_forward(PEER_2, header);
        assert_eq!(result, Some(vec![PEER_1, PEER_2]));
    }

    #[test]
    fn direct_mode_block_announcers_query() {
        let mut handler = BlockFlowHandler::Direct {
            chain: make_test_chain_state(),
        };

        let header = Header {
            hash: BLOCK_HASH_A,
            slot: 100,
            number: 100,
            bytes: vec![],
            era: acropolis_common::Era::Conway,
            parent_hash: Some(GENESIS_HASH),
        };

        handler.handle_roll_forward(PEER_1, header.clone());
        handler.handle_roll_forward(PEER_2, header);

        if let Some(announcers) = handler.block_announcers(100, BLOCK_HASH_A) {
            assert_eq!(announcers, vec![PEER_1, PEER_2]);
        }
    }

    #[test]
    fn block_fetched_moves_peers_to_fetched() {
        let mut tracker = BlockTracker::new();
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, 1, BLOCK_HASH_A, GENESIS_HASH);

        tracker.block_fetched(100, BLOCK_HASH_A);

        assert!(
            !tracker.blocks.contains_key(&(100, BLOCK_HASH_A)),
            "block removed from tracker"
        );
        let peers = tracker.take_rejected_announcers(BLOCK_HASH_A);
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&PEER_1));
        assert!(peers.contains(&PEER_2));
    }

    #[test]
    fn block_fetched_on_removed_block_yields_no_announcers() {
        let mut tracker = BlockTracker::new();
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.handle_rollback(PEER_1, &point(50, GENESIS_HASH));
        let _ = tracker.take_events(); // consume Rescinded

        tracker.block_fetched(100, BLOCK_HASH_A);

        let peers = tracker.take_rejected_announcers(BLOCK_HASH_A);
        assert!(peers.is_empty(), "expected empty for already-removed block");
    }

    #[test]
    fn fetched_block_still_provides_announcers_for_rewant() {
        let mut tracker = BlockTracker::new();
        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.block_fetched(100, BLOCK_HASH_A);

        let announcers = tracker.announcers(100, BLOCK_HASH_A);
        assert_eq!(announcers, vec![PEER_1]);
    }

    #[test]
    fn rollback_removes_peer_from_fetched_entries() {
        let mut tracker = BlockTracker::new();
        tracker.handle_tip(PEER_1, point(120, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 120, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.block_fetched(120, BLOCK_HASH_A);

        tracker.handle_rollback(PEER_1, &point(100, GENESIS_HASH));
        tracker.handle_tip(PEER_1, point(100, GENESIS_HASH));

        assert!(tracker.announcers(120, BLOCK_HASH_A).is_empty());
    }

    #[test]
    fn disconnect_removes_peer_from_fetched_entries() {
        let mut tracker = BlockTracker::new();
        tracker.handle_tip(PEER_1, point(120, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 120, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.block_fetched(120, BLOCK_HASH_A);

        tracker.handle_disconnect(PEER_1);

        assert!(tracker.announcers(120, BLOCK_HASH_A).is_empty());
    }

    #[test]
    fn rollback_last_announcer_emits_block_rescinded() {
        let mut tracker = BlockTracker::new();
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        let _ = tracker.take_events(); // consume Offered

        tracker.handle_rollback(PEER_1, &point(50, GENESIS_HASH));

        let events = tracker.take_events();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ConsensusBlockEvent::Rescinded { hash, slot }
                if *hash == BLOCK_HASH_A && *slot == 100),
            "expected Rescinded for BLOCK_HASH_A at slot 100, got {:?}",
            events
        );
    }

    #[test]
    fn rollback_non_last_announcer_emits_no_block_rescinded() {
        let mut tracker = BlockTracker::new();
        tracker.handle_tip(PEER_1, point(200, BLOCK_HASH_B));
        tracker.handle_tip(PEER_2, point(200, BLOCK_HASH_B));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        let _ = tracker.take_events();

        tracker.handle_rollback(PEER_1, &point(50, GENESIS_HASH));

        let events = tracker.take_events();
        assert!(
            events.is_empty(),
            "expected no events when peer 2 still announces the block, got {:?}",
            events
        );
    }

    #[test]
    fn disconnect_last_announcer_emits_block_rescinded() {
        let mut tracker = BlockTracker::new();
        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        let _ = tracker.take_events();

        tracker.handle_disconnect(PEER_1);

        let events = tracker.take_events();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ConsensusBlockEvent::Rescinded { hash, slot }
                if *hash == BLOCK_HASH_A && *slot == 100),
            "expected Rescinded for BLOCK_HASH_A at slot 100, got {:?}",
            events
        );
    }

    #[test]
    fn two_peer_rollback_second_emits_block_rescinded() {
        let mut tracker = BlockTracker::new();
        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));
        tracker.handle_tip(PEER_2, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        let _ = tracker.take_events();

        tracker.handle_rollback(PEER_1, &point(50, GENESIS_HASH));
        let events_after_first = tracker.take_events();
        assert!(
            events_after_first.is_empty(),
            "first rollback should not rescind (peer 2 still has it), got {:?}",
            events_after_first
        );

        tracker.handle_rollback(PEER_2, &point(50, GENESIS_HASH));
        let events_after_second = tracker.take_events();
        assert_eq!(events_after_second.len(), 1);
        assert!(
            matches!(&events_after_second[0], ConsensusBlockEvent::Rescinded { hash, .. }
                if *hash == BLOCK_HASH_A),
            "second rollback should rescind, got {:?}",
            events_after_second
        );
    }

    #[test]
    fn rollback_can_rescind_multiple_blocks() {
        let mut tracker = BlockTracker::new();
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_1, 101, 2, BLOCK_HASH_B, BLOCK_HASH_A);
        let _ = tracker.take_events();

        // Roll back to before both blocks
        tracker.handle_rollback(PEER_1, &point(50, GENESIS_HASH));

        let events = tracker.take_events();
        assert_eq!(
            events.len(),
            2,
            "both blocks should be rescinded, got {:?}",
            events
        );
        assert!(events.iter().any(
            |e| matches!(e, ConsensusBlockEvent::Rescinded { hash, .. } if *hash == BLOCK_HASH_A)
        ));
        assert!(events.iter().any(
            |e| matches!(e, ConsensusBlockEvent::Rescinded { hash, .. } if *hash == BLOCK_HASH_B)
        ));
    }

    #[test]
    fn direct_mode_block_rejected_announcers_is_empty() {
        let mut handler = BlockFlowHandler::Direct {
            chain: make_test_chain_state(),
        };
        let announcers = handler.block_rejected_announcers(BLOCK_HASH_A);
        assert!(
            announcers.is_empty(),
            "Direct mode should return empty for block_rejected_announcers"
        );
    }

    #[test]
    fn lagging_peer_does_not_re_offer_already_fetched_blocks() {
        let mut tracker = BlockTracker::new();

        // Peer 1 announces blocks 1..3 and they get fetched
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_1, 200, 2, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.track_announcement(PEER_1, 300, 3, BLOCK_HASH_C, BLOCK_HASH_B);
        let initial_events = tracker.take_events();
        assert_eq!(initial_events.len(), 3, "3 offers from peer 1");

        tracker.block_fetched(100, BLOCK_HASH_A);
        tracker.block_fetched(200, BLOCK_HASH_B);
        tracker.block_fetched(300, BLOCK_HASH_C);

        // Peer 2 now announces the same blocks (it's behind)
        tracker.track_announcement(PEER_2, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 200, 2, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.track_announcement(PEER_2, 300, 3, BLOCK_HASH_C, BLOCK_HASH_B);

        let re_offer_events = tracker.take_events();
        assert!(
            re_offer_events.is_empty(),
            "Already-fetched blocks should not be re-offered, got {:?}",
            re_offer_events
        );
    }

    #[test]
    fn lagging_peer_adds_itself_as_announcer_for_fetched_block() {
        let mut tracker = BlockTracker::new();

        // Peer 1 announces and block gets fetched
        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, 1, BLOCK_HASH_A, GENESIS_HASH);
        let _ = tracker.take_events();
        tracker.block_fetched(100, BLOCK_HASH_A);

        // Peer 2 announces the same block later
        tracker.handle_tip(PEER_2, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_2, 100, 1, BLOCK_HASH_A, GENESIS_HASH);

        // No new Offered event
        let events = tracker.take_events();
        assert!(events.is_empty(), "expected no re-offer, got {:?}", events);

        // But peer 2 should appear as an announcer (for future sanctioning)
        let announcers = tracker.announcers(100, BLOCK_HASH_A);
        assert!(
            announcers.contains(&PEER_2),
            "peer 2 should be an announcer for the fetched block, got {:?}",
            announcers
        );
    }

    #[test]
    fn lagging_peer_does_not_flood_offers_during_fast_sync() {
        let mut tracker = BlockTracker::new();

        // Simulate fast sync: peer 1 announces a chain of 5 blocks, all fetched
        let hashes = [
            BLOCK_HASH_A,
            BLOCK_HASH_B,
            BLOCK_HASH_C,
            BLOCK_HASH_D,
            BLOCK_HASH_E,
        ];
        let mut parent = GENESIS_HASH;
        for (i, &hash) in hashes.iter().enumerate() {
            let slot = (i as u64 + 1) * 100;
            let number = i as u64 + 1;
            tracker.track_announcement(PEER_1, slot, number, hash, parent);
            tracker.block_fetched(slot, hash);
            parent = hash;
        }
        let _ = tracker.take_events(); // consume initial offers

        // Peer 2 connects and starts announcing from the beginning
        parent = GENESIS_HASH;
        let mut re_offers = 0;
        for (i, &hash) in hashes.iter().enumerate() {
            let slot = (i as u64 + 1) * 100;
            let number = i as u64 + 1;
            tracker.track_announcement(PEER_2, slot, number, hash, parent);
            re_offers += tracker.take_events().len();
            parent = hash;
        }

        assert_eq!(
            re_offers, 0,
            "lagging peer should produce zero re-offers for already-fetched blocks"
        );
    }

    // Consensus mode intersection point tests

    fn make_test_consensus_state() -> ConsensusFlowState {
        use caryatid_sdk::mock_bus::MockBus;
        use tokio::sync::watch;

        let config = Arc::new(config::Config::builder().build().unwrap());
        let bus = Arc::new(MockBus::<Message>::new(&config));
        let (_tx, rx) = watch::channel(true);
        let context = Arc::new(Context::new(config, bus, rx));
        let k = Arc::new(AtomicU64::new(SECURITY_PARAMETER_K));
        ConsensusFlowState::new(context, "test.consensus.offers".to_string(), k)
    }

    fn hash_for_slot(slot: u64) -> BlockHash {
        use pallas::crypto::hash::Hasher;
        BlockHash::new(*Hasher::<256>::hash(&slot.to_be_bytes()))
    }

    #[test]
    fn consensus_choose_points_empty_when_no_published() {
        let state = make_test_consensus_state();
        assert!(state.choose_points_for_find_intersect().is_empty());
    }

    #[test]
    fn consensus_choose_points_returns_recent_published() {
        let mut state = make_test_consensus_state();

        for slot in 1..=10u64 {
            state.published_points.push_back(SpecificPoint {
                slot,
                hash: hash_for_slot(slot),
            });
        }

        let points = state.choose_points_for_find_intersect();
        assert!(!points.is_empty());

        // Most recent 5 should come first
        if let Point::Specific(slot, _) = &points[0] {
            assert_eq!(*slot, 10); // most recently published
        } else {
            panic!("expected Specific point");
        }
        if let Point::Specific(slot, _) = &points[4] {
            assert_eq!(*slot, 6); // 5th most recent (last in the recent window)
        } else {
            panic!("expected Specific point");
        }
    }

    #[test]
    fn consensus_published_points_capped_at_k() {
        let mut state = make_test_consensus_state();
        let k = state.security_param_k.load(Ordering::Acquire);

        for slot in 1..=(k + 100) {
            state.published_points.push_back(SpecificPoint {
                slot,
                hash: hash_for_slot(slot),
            });
            while state.published_points.len() > k as usize {
                state.published_points.pop_front();
            }
        }

        assert_eq!(state.published_points.len(), k as usize);
        assert_eq!(state.published_points.front().unwrap().slot, 101);
    }

    #[test]
    fn consensus_reset_clears_published_points() {
        let mut state = make_test_consensus_state();

        for slot in 1..=20u64 {
            state.published_points.push_back(SpecificPoint {
                slot,
                hash: hash_for_slot(slot),
            });
        }
        assert!(!state.published_points.is_empty());

        state.handle_sync_reset();

        assert!(state.published_points.is_empty());
        assert!(state.choose_points_for_find_intersect().is_empty());
    }

    #[test]
    fn consensus_handle_new_connection_uses_published_points() {
        use caryatid_sdk::mock_bus::MockBus;
        use tokio::sync::watch;

        let config = Arc::new(config::Config::builder().build().unwrap());
        let bus = Arc::new(MockBus::<Message>::new(&config));
        let (_tx, rx) = watch::channel(true);
        let context = Arc::new(Context::new(config, bus, rx));
        let k = Arc::new(AtomicU64::new(SECURITY_PARAMETER_K));

        let mut state = ConsensusFlowState::new(context, "test.topic".to_string(), k);
        for slot in 100..=120u64 {
            state.published_points.push_back(SpecificPoint {
                slot,
                hash: hash_for_slot(slot),
            });
        }

        let mut handler = BlockFlowHandler::Consensus(state);
        let stale_point = Point::Specific(1, vec![0; 32]);
        let points = handler.handle_new_connection(PEER_1, Some(&stale_point));

        // Should use published_points, not the stale sync_point
        assert!(!points.is_empty());
        if let Point::Specific(slot, _) = &points[0] {
            assert_eq!(*slot, 120, "most recent published point should be first");
        } else {
            panic!("expected Specific point");
        }
    }

    #[test]
    fn consensus_handle_new_connection_falls_back_to_sync_point_when_empty() {
        let state = make_test_consensus_state();
        let mut handler = BlockFlowHandler::Consensus(state);
        let sync_point = Point::Specific(42, vec![0xAA; 32]);
        let points = handler.handle_new_connection(PEER_1, Some(&sync_point));

        assert_eq!(points.len(), 1);
        assert_eq!(points[0], sync_point);
    }

    #[test]
    fn rollback_prunes_published_points() {
        let mut state = make_test_consensus_state();

        for slot in 1..=10u64 {
            state.published_points.push_back(SpecificPoint {
                slot,
                hash: hash_for_slot(slot),
            });
        }
        assert_eq!(state.published_points.len(), 10);

        let rollback_hash = hash_for_slot(5);
        state.handle_roll_backward(PEER_1, Point::Specific(5, rollback_hash.to_vec()));

        assert_eq!(
            state.published_points.len(),
            5,
            "published_points should be pruned to slots 1..=5 after rollback to slot 5"
        );
        assert_eq!(state.published_points.back().unwrap().slot, 5);
    }
}
