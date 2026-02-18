use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use acropolis_common::BlockHash;
use acropolis_common::messages::{
    BlockOfferedMessage, BlockWantedMessage, ConsensusMessage, Message,
};
use anyhow::Result;
use caryatid_sdk::{Context, Subscription};
use pallas::network::miniprotocols::Point;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::BlockSink;
use crate::chain_state::{ChainEvent, ChainState};
use crate::configuration::{BlockFlowMode, InterfaceConfig};
use crate::connection::Header;
use crate::network::{NetworkEvent, PeerId};

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
        context: Arc<Context<Message>>,
        events_sender: mpsc::Sender<NetworkEvent>,
    ) -> Result<Self> {
        match config.block_flow_mode {
            BlockFlowMode::Direct => {
                info!("Block flow mode: Direct (auto-fetch)");
                Ok(BlockFlowHandler::Direct {
                    chain: ChainState::new(),
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
                )))
            }
        }
    }

    async fn forward_block_wanted_to_events(
        mut subscription: Box<dyn Subscription<Message>>,
        events_sender: mpsc::Sender<NetworkEvent>,
    ) {
        while let Ok((_, msg)) = subscription.read().await {
            if let Message::Consensus(ConsensusMessage::BlockWanted(BlockWantedMessage {
                hash,
                slot,
            })) = msg.as_ref()
                && events_sender
                    .send(NetworkEvent::BlockWanted {
                        hash: *hash,
                        slot: *slot,
                    })
                    .await
                    .is_err()
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
            BlockFlowHandler::Consensus(state) => state.handle_block_fetched(slot, hash),
        }
    }

    /// Whether PNI should autonomously re-request blocks when a peer disconnects.
    /// In Direct mode, PNI manages chain selection and should retry from other peers.
    /// In Consensus mode, block fetching is driven by the consensus module via BlockWanted.
    pub fn should_rerequest_on_disconnect(&self) -> bool {
        matches!(self, BlockFlowHandler::Direct { .. })
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
    /// In Direct mode, uses chain state points (or falls back to sync_point),
    /// and sets preferred upstream if none is set.
    /// In Consensus mode, just uses sync_point.
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
            BlockFlowHandler::Consensus(_) => {}
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

    /// Reset state for a new sync point.
    pub fn handle_sync_reset(&mut self) {
        match self {
            BlockFlowHandler::Direct { chain } => *chain = ChainState::new(),
            BlockFlowHandler::Consensus(state) => state.handle_sync_reset(),
        }
    }

    /// Publish events appropriate for the current flow mode.
    ///
    /// - Direct mode: publishes RollForward/RollBackward from chain state
    /// - Consensus mode: publishes BlockOffered/BlockRescinded to consensus
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
                state.publish_pending().await?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConsensusEvent {
    BlockOffered {
        hash: BlockHash,
        slot: u64,
        parent_hash: BlockHash,
    },
    // TODO: BlockRescinded would be sent when NO peers have a block anymore.
    // TODO: This requires tracking when all announcers disconnect/rollback - not implemented yet.
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
    pending_events: Vec<ConsensusEvent>,
}

impl BlockTracker {
    fn new() -> Self {
        Self::default()
    }

    /// Record a block announcement from a peer.
    /// Generates a BlockOffered event if this is the first announcement for this block.
    fn track_announcement(
        &mut self,
        peer: PeerId,
        slot: u64,
        hash: BlockHash,
        parent_hash: BlockHash,
    ) {
        let is_new = !self.blocks.contains_key(&(slot, hash));
        let announcers = self.blocks.entry((slot, hash)).or_default();
        if !announcers.contains(&peer) {
            announcers.push(peer);
        }
        if is_new {
            self.pending_events.push(ConsensusEvent::BlockOffered {
                hash,
                slot,
                parent_hash,
            });
        }
    }

    /// Update a peer's known tip.
    fn handle_tip(&mut self, peer: PeerId, tip: Point) {
        self.tips.insert(peer, tip);
    }

    /// Handle a peer rolling back — remove it from blocks beyond the rollback point.
    fn handle_rollback(&mut self, peer: PeerId, point: &Point) {
        let rollback_to_slot = match point {
            Point::Origin => 0,
            Point::Specific(slot, _) => *slot,
        };
        for announcers in self.blocks.range_mut((rollback_to_slot + 1, BlockHash::default())..) {
            announcers.1.retain(|p| *p != peer);
        }
        self.blocks.retain(|_, announcers| !announcers.is_empty());
    }

    /// Handle a peer disconnecting — remove it from all blocks and tips.
    fn handle_disconnect(&mut self, peer: PeerId) {
        self.tips.remove(&peer);
        for announcers in self.blocks.values_mut() {
            announcers.retain(|p| *p != peer);
        }
        self.blocks.retain(|_, announcers| !announcers.is_empty());
    }

    /// Checks if a peer's tip is at or beyond the given slot.
    fn peer_can_have_block(&self, peer: PeerId, slot: u64) -> bool {
        match self.tips.get(&peer) {
            Some(Point::Specific(peer_slot, _)) => *peer_slot >= slot,
            Some(Point::Origin) | None => false,
        }
    }

    /// Get peers that announced a block and whose tip still covers it.
    fn announcers(&self, slot: u64, hash: BlockHash) -> Vec<PeerId> {
        self.blocks
            .get(&(slot, hash))
            .into_iter()
            .flat_map(|peers| peers.iter())
            .filter(|peer| self.peer_can_have_block(**peer, slot))
            .copied()
            .collect()
    }

    /// A block was successfully fetched — remove it from tracking.
    fn block_fetched(&mut self, slot: u64, hash: BlockHash) {
        self.blocks.remove(&(slot, hash));
    }

    /// Take all pending events for publishing.
    fn take_events(&mut self) -> Vec<ConsensusEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Clear all state.
    fn reset(&mut self) {
        self.blocks.clear();
        self.tips.clear();
        self.pending_events.clear();
    }
}

pub struct ConsensusFlowState {
    context: Arc<Context<Message>>,
    topic: String,
    tracker: BlockTracker,
    blocks_offered_count: u64,
}

impl ConsensusFlowState {
    fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self {
            context,
            topic,
            tracker: BlockTracker::new(),
            blocks_offered_count: 0,
        }
    }

    fn handle_roll_forward(&mut self, peer: PeerId, header: &Header) {
        let parent_hash = header.parent_hash.unwrap_or_default();
        self.tracker.track_announcement(peer, header.slot, header.hash, parent_hash);
    }

    fn handle_roll_backward(&mut self, peer: PeerId, point: Point) {
        self.tracker.handle_rollback(peer, &point);
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

    async fn publish_pending(&mut self) -> Result<()> {
        for event in self.tracker.take_events() {
            let consensus_message = match event {
                ConsensusEvent::BlockOffered {
                    hash,
                    slot,
                    parent_hash,
                } => {
                    self.blocks_offered_count += 1;
                    if self.blocks_offered_count.is_multiple_of(100) {
                        info!("Offered block (consensus) {}", hash);
                    }

                    ConsensusMessage::BlockOffered(BlockOfferedMessage {
                        hash,
                        slot,
                        parent_hash,
                    })
                }
            };

            let message = Arc::new(Message::Consensus(consensus_message));

            if let Err(e) = self.context.publish(&self.topic, message).await {
                error!("Failed to publish consensus event: {e}");
                continue;
            }
        }
        Ok(())
    }

    fn handle_block_fetched(&mut self, slot: u64, hash: BlockHash) {
        self.tracker.block_fetched(slot, hash);
    }

    fn handle_sync_reset(&mut self) {
        self.tracker.reset();
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

    fn point(slot: u64, hash: BlockHash) -> Point {
        Point::Specific(slot, hash.to_vec())
    }

    #[test]
    fn first_announcement_emits_offer() {
        let mut tracker = BlockTracker::new();

        tracker.track_announcement(PEER_1, 100, BLOCK_HASH_A, GENESIS_HASH);

        assert!(tracker.blocks.contains_key(&(100, BLOCK_HASH_A)));
        let events = tracker.take_events();
        assert!(matches!(
            &events[..],
            [ConsensusEvent::BlockOffered { slot: 100, hash, parent_hash }]
                if *hash == BLOCK_HASH_A && *parent_hash == GENESIS_HASH
        ));
    }

    #[test]
    fn duplicate_announcement_emits_single_offer() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));
        tracker.handle_tip(PEER_2, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, BLOCK_HASH_A, GENESIS_HASH);

        assert_eq!(tracker.blocks.len(), 1);
        assert_eq!(tracker.announcers(100, BLOCK_HASH_A).len(), 2);
        assert_eq!(tracker.take_events().len(), 1);
    }

    #[test]
    fn fork_at_same_slot_tracks_both_blocks() {
        let mut tracker = BlockTracker::new();

        tracker.track_announcement(PEER_1, 100, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, BLOCK_HASH_B, GENESIS_HASH);

        assert_eq!(tracker.blocks.len(), 2);
        assert_eq!(tracker.take_events().len(), 2);
    }

    #[test]
    fn block_fetched_removes_from_tracking() {
        let mut tracker = BlockTracker::new();

        tracker.track_announcement(PEER_1, 100, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_1, 101, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.block_fetched(100, BLOCK_HASH_A);

        assert!(!tracker.blocks.contains_key(&(100, BLOCK_HASH_A)));
        assert!(tracker.blocks.contains_key(&(101, BLOCK_HASH_B)));
    }

    #[test]
    fn rollback_removes_peer_beyond_slot() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(200, BLOCK_HASH_B));
        tracker.handle_tip(PEER_2, point(200, BLOCK_HASH_B));
        tracker.track_announcement(PEER_1, 100, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_1, 200, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.track_announcement(PEER_2, 200, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.handle_rollback(PEER_1, &point(100, BLOCK_HASH_A));
        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));

        assert_eq!(tracker.announcers(100, BLOCK_HASH_A), vec![PEER_1]);
        assert_eq!(tracker.announcers(200, BLOCK_HASH_B), vec![PEER_2]);
    }

    #[test]
    fn disconnect_removes_peer_from_all_blocks() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(200, BLOCK_HASH_B));
        tracker.handle_tip(PEER_2, point(200, BLOCK_HASH_B));
        tracker.track_announcement(PEER_1, 100, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_1, 200, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.track_announcement(PEER_2, 200, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.handle_disconnect(PEER_1);

        assert!(tracker.announcers(100, BLOCK_HASH_A).is_empty());
        assert_eq!(tracker.announcers(200, BLOCK_HASH_B), vec![PEER_2]);
    }

    #[test]
    fn peer_without_tip_excluded_from_announcers() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_2, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, BLOCK_HASH_A, GENESIS_HASH);
        tracker.track_announcement(PEER_2, 100, BLOCK_HASH_A, GENESIS_HASH);

        assert_eq!(tracker.announcers(100, BLOCK_HASH_A), vec![PEER_2]);
    }

    #[test]
    fn peer_with_stale_tip_excluded_from_announcers() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(200, BLOCK_HASH_B));
        tracker.track_announcement(PEER_1, 200, BLOCK_HASH_B, BLOCK_HASH_A);
        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));

        assert!(tracker.announcers(200, BLOCK_HASH_B).is_empty());
    }

    #[test]
    fn reset_clears_all_state() {
        let mut tracker = BlockTracker::new();

        tracker.handle_tip(PEER_1, point(100, BLOCK_HASH_A));
        tracker.track_announcement(PEER_1, 100, BLOCK_HASH_A, GENESIS_HASH);
        tracker.reset();

        assert!(tracker.blocks.is_empty());
        assert!(tracker.tips.is_empty());
        assert!(tracker.take_events().is_empty());
    }

    #[test]
    fn direct_mode_tracks_and_returns_announcers() {
        let mut handler = BlockFlowHandler::Direct {
            chain: ChainState::new(),
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
            chain: ChainState::new(),
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
}
