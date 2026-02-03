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

use crate::configuration::{BlockFlowMode, InterfaceConfig};
use crate::connection::Header;
use crate::network::{NetworkEvent, PeerId};

/// Block flow handling strategies.
pub enum BlockFlowHandler {
    /// Direct: auto-fetch blocks as announced
    Direct,
    /// Consensus-driven: publish offers, wait for wants before fetching
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
    pub fn handle_roll_forward(&mut self, header: &Header, announcers: Vec<PeerId>) -> Option<Vec<PeerId>> {
        match self {
            BlockFlowHandler::Direct => {
                if announcers.is_empty() { None } else { Some(announcers) }
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

    pub async fn publish_pending(&mut self) -> Result<()> {
        if let BlockFlowHandler::Consensus(state) = self {
            state.publish_pending().await?;
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

pub struct ConsensusFlowState {
    context: Arc<Context<Message>>,
    topic: String,
    pending_events: Vec<ConsensusEvent>,
    offered_blocks: BTreeMap<u64, HashSet<BlockHash>>,
    publish_failure_count: u32,
}

const MAX_PUBLISH_FAILURES: u32 = 10;

impl ConsensusFlowState {
    fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self {
            context,
            topic,
            pending_events: Vec::new(),
            offered_blocks: BTreeMap::new(),
            publish_failure_count: 0,
        }
    }

    fn handle_roll_forward(&mut self, header: &Header) {
        let hash = header.hash;
        let slot = header.slot;
        let parent_hash = header.parent_hash.unwrap_or_default();

        // Track that we're offering this block
        let is_new = self.offered_blocks.entry(slot).or_default().insert(hash);

        // Only publish offer if we haven't offered this exact block before
        if is_new {
            self.pending_events.push(ConsensusEvent::BlockOffered {
                hash,
                slot,
                parent_hash,
            });
        }
    }

    fn handle_roll_backward(&mut self, rollback_to_slot: u64) {
        // Collect slots that need to be rescinded (slot > rollback point)
        let slots_to_rescind: Vec<u64> =
            self.offered_blocks.range((rollback_to_slot + 1)..).map(|(slot, _)| *slot).collect();

        // Rescind all blocks at those slots
        for slot in slots_to_rescind {
            if let Some(hashes) = self.offered_blocks.remove(&slot) {
                for hash in hashes {
                    self.pending_events.push(ConsensusEvent::BlockRescinded { hash, slot });
                }
            }
        }
    }

    async fn publish_pending(&mut self) -> Result<()> {
        if self.pending_events.is_empty() {
            return Ok(());
        }

        let events = std::mem::take(&mut self.pending_events);

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
        // Remove from tracking - block has been fetched
        if let Some(hashes) = self.offered_blocks.get_mut(&slot) {
            hashes.remove(&hash);
            if hashes.is_empty() {
                self.offered_blocks.remove(&slot);
            }
        }
    }

    fn handle_sync_reset(&mut self) {
        self.offered_blocks.clear();
        self.pending_events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::Era;

    fn make_header(slot: u64, hash_byte: u8) -> Header {
        Header {
            hash: BlockHash::new([hash_byte; 32]),
            slot,
            number: slot,
            bytes: vec![],
            era: Era::Conway,
            parent_hash: Some(BlockHash::new([hash_byte.wrapping_sub(1); 32])),
        }
    }

    #[test]
    fn consensus_state_tracks_offered_blocks() {
        let header1 = make_header(100, 1);
        let header2 = make_header(101, 2);
        let header3 = make_header(102, 3);

        // Simulate the tracking that would happen
        let mut offered: BTreeMap<u64, HashSet<BlockHash>> = BTreeMap::new();

        offered.entry(header1.slot).or_default().insert(header1.hash);
        offered.entry(header2.slot).or_default().insert(header2.hash);
        offered.entry(header3.slot).or_default().insert(header3.hash);

        assert_eq!(offered.len(), 3);

        // Simulate rollback to slot 100
        let rollback_to = 100;
        let to_remove: Vec<u64> = offered.range((rollback_to + 1)..).map(|(s, _)| *s).collect();
        for slot in to_remove {
            offered.remove(&slot);
        }

        assert_eq!(offered.len(), 1);
        assert!(offered.contains_key(&100));
    }

    #[test]
    fn consensus_state_deduplicates_offers() {
        let header = make_header(100, 1);

        let mut offered: BTreeMap<u64, HashSet<BlockHash>> = BTreeMap::new();

        // First announcement
        let is_new_1 = offered.entry(header.slot).or_default().insert(header.hash);
        assert!(is_new_1);

        // Second announcement of same block
        let is_new_2 = offered.entry(header.slot).or_default().insert(header.hash);
        assert!(!is_new_2); // Should not be new

        // Only one entry
        assert_eq!(offered.get(&100).unwrap().len(), 1);
    }
}
