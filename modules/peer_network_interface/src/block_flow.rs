use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use acropolis_common::BlockHash;
use acropolis_common::messages::{
    BlockOfferedMessage, BlockRescindedMessage, BlockWantedMessage, ConsensusMessage, Message,
};
use anyhow::{Result, bail};
use caryatid_sdk::{Context, Subscription};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::BlockSink;
use crate::chain_state::{ChainEvent, ChainState};
use crate::configuration::{BlockFlowMode, InterfaceConfig};
use crate::connection::Header;
use crate::network::{NetworkEvent, PeerId};

/// Block flow handling strategies.
pub enum BlockFlowHandler {
    /// Direct: auto-fetch blocks as announced, PNI manages chain selection
    Direct,
    /// Consensus-driven: publish offers, wait for wants before fetching.
    /// Chain selection is delegated to the consensus module.
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
                Ok(BlockFlowHandler::Direct)
            }
            BlockFlowMode::Consensus => {
                info!(
                    "Block flow mode: Consensus (offers on '{}', wants on '{}')",
                    config.consensus_topic, config.block_wanted_topic
                );
                let subscription = context.subscribe(&config.block_wanted_topic).await?;
                tokio::spawn(Self::forward_block_wanted_to_events(
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
    ) -> Result<()> {
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
                bail!("event channel closed");
            }
        }
        bail!("subscription closed");
    }

    /// Handle a new block announcement. Returns peers to fetch from, or None if awaiting consensus.
    pub fn handle_roll_forward(
        &mut self,
        header: &Header,
        announcers: Vec<PeerId>,
    ) -> Option<Vec<PeerId>> {
        match self {
            BlockFlowHandler::Direct => {
                if announcers.is_empty() {
                    None
                } else {
                    Some(announcers)
                }
            }
            BlockFlowHandler::Consensus(state) => {
                state.handle_roll_forward(header);
                None
            }
        }
    }

    pub fn handle_roll_backward(&mut self, rollback_to_slot: u64) {
        if let BlockFlowHandler::Consensus(state) = self {
            state.handle_roll_backward(rollback_to_slot);
        }
    }

    /// Publish events appropriate for the current flow mode.
    ///
    /// - Direct mode: publishes RollForward/RollBackward from chain state
    /// - Consensus mode: publishes BlockOffered/BlockRescinded to consensus
    pub async fn publish(
        &mut self,
        chain: &mut ChainState,
        block_sink: &mut BlockSink,
        published_blocks: &mut u64,
    ) -> Result<()> {
        match self {
            BlockFlowHandler::Direct => {
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

    pub fn handle_block_fetched(&mut self, slot: u64, hash: BlockHash) {
        if let BlockFlowHandler::Consensus(state) = self {
            state.handle_block_fetched(slot, hash);
        }
    }

    pub fn handle_sync_reset(&mut self) {
        if let BlockFlowHandler::Consensus(state) = self {
            state.handle_sync_reset();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConsensusEvent {
    BlockOffered {
        hash: BlockHash,
        slot: u64,
        parent_hash: BlockHash,
    },
    BlockRescinded {
        hash: BlockHash,
        slot: u64,
    },
}

/// Tracks block offers and generates consensus events.
#[derive(Default)]
struct BlockOfferTracker {
    pending_events: Vec<ConsensusEvent>,
    offered_blocks: BTreeMap<u64, HashSet<BlockHash>>,
}

impl BlockOfferTracker {
    fn new() -> Self {
        Self::default()
    }

    /// Record a new block announcement and produce BlockOffered event if this
    /// is the first 'new block'.
    fn roll_forward(&mut self, slot: u64, hash: BlockHash, parent_hash: BlockHash) {
        let is_new = self.offered_blocks.entry(slot).or_default().insert(hash);
        if is_new {
            self.pending_events.push(ConsensusEvent::BlockOffered {
                hash,
                slot,
                parent_hash,
            });
        }
    }

    /// Handle a rollback, produces BlockRescinded events for all blocks
    /// at slots strictly greater than the rollback point.
    fn roll_backward(&mut self, rollback_to_slot: u64) {
        // Collect slots beyond the rollback point
        let slots_to_rescind: Vec<u64> =
            self.offered_blocks.range((rollback_to_slot + 1)..).map(|(slot, _)| *slot).collect();

        for slot in slots_to_rescind {
            if let Some(hashes) = self.offered_blocks.remove(&slot) {
                for hash in hashes {
                    self.pending_events.push(ConsensusEvent::BlockRescinded { hash, slot });
                }
            }
        }
    }

    /// A block was successfully fetched - remove it from tracking.
    fn block_fetched(&mut self, slot: u64, hash: BlockHash) {
        if let Some(hashes) = self.offered_blocks.get_mut(&slot) {
            hashes.remove(&hash);
            if hashes.is_empty() {
                self.offered_blocks.remove(&slot);
            }
        }
    }

    /// Clear all state (used on sync reset).
    fn reset(&mut self) {
        self.offered_blocks.clear();
        self.pending_events.clear();
    }

    /// Take all pending events for publishing.
    fn take_events(&mut self) -> Vec<ConsensusEvent> {
        std::mem::take(&mut self.pending_events)
    }
}

pub struct ConsensusFlowState {
    context: Arc<Context<Message>>,
    topic: String,
    tracker: BlockOfferTracker,
    publish_failure_count: u32,
}

const MAX_PUBLISH_FAILURES: u32 = 10;

impl ConsensusFlowState {
    fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self {
            context,
            topic,
            tracker: BlockOfferTracker::new(),
            publish_failure_count: 0,
        }
    }

    fn handle_roll_forward(&mut self, header: &Header) {
        let parent_hash = header.parent_hash.unwrap_or_default();
        self.tracker.roll_forward(header.slot, header.hash, parent_hash);
    }

    fn handle_roll_backward(&mut self, rollback_to_slot: u64) {
        self.tracker.roll_backward(rollback_to_slot);
    }

    async fn publish_pending(&mut self) -> Result<()> {
        let events = self.tracker.take_events();
        if events.is_empty() {
            return Ok(());
        }

        for event in events {
            let msg = match event {
                ConsensusEvent::BlockOffered {
                    hash,
                    slot,
                    parent_hash,
                } => ConsensusMessage::BlockOffered(BlockOfferedMessage {
                    hash,
                    slot,
                    parent_hash,
                }),
                ConsensusEvent::BlockRescinded { hash, slot } => {
                    ConsensusMessage::BlockRescinded(BlockRescindedMessage { hash, slot })
                }
            };

            let message = Arc::new(Message::Consensus(msg));
            if let Err(e) = self.context.publish(&self.topic, message).await {
                self.publish_failure_count += 1;
                if self.publish_failure_count >= MAX_PUBLISH_FAILURES {
                    error!(
                        "Failed to publish consensus event after {} attempts: {e}. \
                         Consensus module may be unavailable.",
                        self.publish_failure_count
                    );
                } else {
                    warn!("Failed to publish consensus event: {e}");
                }
            } else {
                self.publish_failure_count = 0;
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

    fn hash(n: u8) -> BlockHash {
        BlockHash::new([n; 32])
    }

    mod offer_tracker {
        use super::*;

        fn has_block(tracker: &BlockOfferTracker, slot: u64, h: BlockHash) -> bool {
            tracker.offered_blocks.get(&slot).is_some_and(|set| set.contains(&h))
        }

        #[test]
        fn roll_forward_tracks_block_and_emits_offer() {
            let mut tracker = BlockOfferTracker::new();

            tracker.roll_forward(100, hash(1), hash(0));

            assert!(has_block(&tracker, 100, hash(1)));
            let events = tracker.take_events();
            assert!(matches!(
                &events[..],
                [ConsensusEvent::BlockOffered { slot: 100, hash, parent_hash }]
                    if *hash == self::hash(1) && *parent_hash == self::hash(0)
            ));
        }

        #[test]
        fn duplicate_announcement_emits_single_offer() {
            let mut tracker = BlockOfferTracker::new();

            tracker.roll_forward(100, hash(1), hash(0));
            tracker.roll_forward(100, hash(1), hash(0));

            assert_eq!(tracker.offered_blocks[&100].len(), 1);
            assert_eq!(tracker.take_events().len(), 1);
        }

        #[test]
        fn fork_at_same_slot_tracks_both_blocks() {
            let mut tracker = BlockOfferTracker::new();

            tracker.roll_forward(100, hash(1), hash(0));
            tracker.roll_forward(100, hash(2), hash(0));

            assert_eq!(tracker.offered_blocks[&100].len(), 2);
            assert_eq!(tracker.take_events().len(), 2);
        }

        #[test]
        fn rollback_rescinds_blocks_beyond_slot() {
            let mut tracker = BlockOfferTracker::new();

            tracker.roll_forward(100, hash(1), hash(0));
            tracker.roll_forward(101, hash(2), hash(1));
            tracker.roll_forward(102, hash(3), hash(2));
            tracker.take_events();

            tracker.roll_backward(100);

            assert!(has_block(&tracker, 100, hash(1)));
            assert!(!has_block(&tracker, 101, hash(2)));
            assert!(!has_block(&tracker, 102, hash(3)));

            let events = tracker.take_events();
            assert_eq!(events.len(), 2);
            assert!(events.iter().all(|e| matches!(e, ConsensusEvent::BlockRescinded { .. })));
        }

        #[test]
        fn block_fetched_removes_from_tracking() {
            let mut tracker = BlockOfferTracker::new();

            tracker.roll_forward(100, hash(1), hash(0));
            tracker.roll_forward(101, hash(2), hash(1));
            tracker.block_fetched(100, hash(1));

            assert!(!has_block(&tracker, 100, hash(1)));
            assert!(has_block(&tracker, 101, hash(2)));
        }

        #[test]
        fn reset_clears_all_state() {
            let mut tracker = BlockOfferTracker::new();

            tracker.roll_forward(100, hash(1), hash(0));
            tracker.reset();

            assert!(tracker.offered_blocks.is_empty());
            assert!(tracker.take_events().is_empty());
        }
    }

    mod block_flow_handler {
        use super::*;
        use crate::network::PeerId;

        #[test]
        fn direct_mode_returns_announcers() {
            let mut handler = BlockFlowHandler::Direct;
            let header = crate::connection::Header {
                hash: hash(1),
                slot: 100,
                number: 100,
                bytes: vec![],
                era: acropolis_common::Era::Conway,
                parent_hash: Some(hash(0)),
            };

            let peers = vec![PeerId(1), PeerId(2)];
            let result = handler.handle_roll_forward(&header, peers.clone());

            assert_eq!(result, Some(peers));
        }

        #[test]
        fn direct_mode_returns_none_for_empty_announcers() {
            let mut handler = BlockFlowHandler::Direct;
            let header = crate::connection::Header {
                hash: hash(1),
                slot: 100,
                number: 100,
                bytes: vec![],
                era: acropolis_common::Era::Conway,
                parent_hash: Some(hash(0)),
            };

            let result = handler.handle_roll_forward(&header, vec![]);

            assert!(result.is_none());
        }
    }
}
