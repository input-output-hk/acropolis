use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::{connection::Header, network::PeerId};
use acropolis_common::{BlockHash, hash::Hash, params::SECURITY_PARAMETER_K};
use pallas::network::miniprotocols::Point;
use tracing::warn;

#[derive(Debug)]
struct BlockData {
    header: Header,
    announced_by: Vec<PeerId>,
    body: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
struct SlotBlockData {
    blocks: Vec<BlockData>,
}
impl SlotBlockData {
    fn track_announcement(&mut self, id: PeerId, header: Header) {
        if let Some(block) = self.blocks.iter_mut().find(|b| b.header.hash == header.hash) {
            block.announced_by.push(id);
        } else {
            self.blocks.push(BlockData {
                header,
                announced_by: vec![id],
                body: None,
            });
        }
    }

    fn track_rollback(&mut self, id: PeerId) -> bool {
        self.blocks.retain_mut(|block| {
            block.announced_by.retain(|p| *p != id);
            !block.announced_by.is_empty()
        });
        !self.blocks.is_empty()
    }

    fn was_hash_announced(&self, id: PeerId, hash: BlockHash) -> bool {
        self.blocks.iter().any(|b| b.header.hash == hash && b.announced_by.contains(&id))
    }

    fn find_announced_hash(&self, id: PeerId) -> Option<BlockHash> {
        self.blocks.iter().find_map(|b| {
            if b.announced_by.contains(&id) {
                Some(b.header.hash)
            } else {
                None
            }
        })
    }

    fn announcers(&self, hash: BlockHash) -> Vec<PeerId> {
        match self.blocks.iter().find(|b| b.header.hash == hash) {
            Some(b) => b.announced_by.clone(),
            None => vec![],
        }
    }

    fn track_body(&mut self, hash: BlockHash, body: Vec<u8>) {
        let Some(block) = self.blocks.iter_mut().find(|b| b.header.hash == hash) else {
            return;
        };
        if block.body.is_none() {
            block.body = Some(body);
        }
    }

    fn header(&self, hash: BlockHash) -> Option<&Header> {
        for block in &self.blocks {
            if block.header.hash != hash {
                continue;
            }
            return Some(&block.header);
        }
        None
    }

    fn body(&self, hash: BlockHash) -> Option<(&Header, &[u8])> {
        for block in &self.blocks {
            if block.header.hash != hash {
                continue;
            }
            return Some((&block.header, block.body.as_ref()?));
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpecificPoint {
    slot: u64,
    hash: BlockHash,
}
impl SpecificPoint {
    fn as_pallas_point(&self) -> Point {
        Point::Specific(self.slot, self.hash.to_vec())
    }
}

#[derive(Debug, Default)]
pub struct ChainState {
    pub preferred_upstream: Option<PeerId>,
    blocks: BTreeMap<u64, SlotBlockData>,
    published_blocks: VecDeque<SpecificPoint>,
    unpublished_blocks: VecDeque<SpecificPoint>,
    rolled_back_to: Option<Header>,
    tips: HashMap<PeerId, Point>,
    waiting_for_first_message: bool,
}

impl ChainState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_roll_forward(&mut self, id: PeerId, header: Header) -> Vec<PeerId> {
        let is_preferred = self.preferred_upstream == Some(id);
        let slot = header.slot;
        let hash = header.hash;
        let slot_blocks = self.blocks.entry(header.slot).or_default();
        slot_blocks.track_announcement(id, header);
        if is_preferred {
            if self.waiting_for_first_message {
                self.switch_head_to_peer(id);
            } else {
                let point = SpecificPoint { slot, hash };
                self.unpublished_blocks.push_back(point);
            }
        }
        self.block_announcers(slot, hash)
    }

    pub fn handle_roll_backward(&mut self, id: PeerId, point: Point) {
        let is_preferred = self.preferred_upstream == Some(id);
        let mut rolled_back = false;
        match point {
            Point::Origin => {
                self.blocks.retain(|_, b| b.track_rollback(id));
                if is_preferred {
                    if !self.published_blocks.is_empty() {
                        rolled_back = true;
                    }
                    self.published_blocks.clear();
                    self.unpublished_blocks.clear();
                }
            }
            Point::Specific(slot, _) => {
                self.blocks.retain(|s, b| *s <= slot || b.track_rollback(id));
                if is_preferred {
                    while let Some(block) = self.unpublished_blocks.back() {
                        if block.slot > slot {
                            self.unpublished_blocks.pop_back();
                        } else {
                            break;
                        }
                    }
                    while let Some(block) = self.published_blocks.back() {
                        if block.slot > slot {
                            rolled_back = true;
                            self.published_blocks.pop_back();
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        if rolled_back {
            self.rolled_back_to = Some(self.build_header_for_rollback(point));
        }
        if is_preferred {
            self.waiting_for_first_message = false;
        }
    }

    // When we roll back to an earlier point on the chain, we may or may not have the header for that point already.
    // We need fields from the header to fully populate BlockInfo for downstream consumers.
    // Build a header with as much accurate information as we have.
    fn build_header_for_rollback(&self, point: Point) -> Header {
        let Point::Specific(slot, hash) = point else {
            return Header {
                hash: Hash::default(),
                slot: 0,
                number: 0,
                bytes: vec![],
                era: acropolis_common::Era::Byron,
                parent_hash: None,
            };
        };
        let hash = Hash::try_from(hash).unwrap_or_default();
        if let Some(slot_blocks) = self.blocks.get(&slot)
            && let Some(header) = slot_blocks.header(hash)
        {
            header.clone()
        } else {
            Header {
                hash,
                slot,
                number: 0,
                bytes: vec![],
                era: acropolis_common::Era::default(),
                parent_hash: None,
            }
        }
    }

    pub fn handle_body_fetched(&mut self, slot: u64, hash: BlockHash, body: Vec<u8>) {
        let Some(slot_blocks) = self.blocks.get_mut(&slot) else {
            return;
        };
        slot_blocks.track_body(hash, body);
    }

    pub fn handle_new_preferred_upstream(&mut self, id: PeerId) {
        if self.preferred_upstream == Some(id) {
            return;
        }
        self.preferred_upstream = Some(id);
        self.switch_head_to_peer(id);
    }

    pub fn handle_tip(&mut self, id: PeerId, tip: Point) {
        self.tips.insert(id, tip);
    }

    pub fn preferred_upstream_tip(&self) -> Option<&Point> {
        self.tips.get(&self.preferred_upstream?)
    }

    pub fn handle_disconnect(&mut self, id: PeerId, next_id: Option<PeerId>) {
        self.tips.remove(&id);
        if self.preferred_upstream == Some(id) {
            self.preferred_upstream = None;

            if let Some(new_preferred) = next_id {
                self.handle_new_preferred_upstream(new_preferred);
            } else {
                warn!("no upstream peers!");
            }
        }
    }

    fn switch_head_to_peer(&mut self, id: PeerId) {
        self.waiting_for_first_message = false;

        // If there are any blocks queued to be published which our preferred upstream never announced,
        // unqueue them now.
        while let Some(block) = self.unpublished_blocks.back() {
            if let Some(slot_blocks) = self.blocks.get(&block.slot)
                && slot_blocks.was_hash_announced(id, block.hash)
            {
                break;
            } else {
                self.unpublished_blocks.pop_back();
            }
        }

        let mut peer_start = None;
        for (slot, slot_blocks) in self.blocks.iter() {
            if let Some(hash) = slot_blocks.find_announced_hash(id) {
                peer_start = Some(SpecificPoint { slot: *slot, hash });
                break;
            }
        }

        let Some(peer_start) = peer_start else {
            // We haven't seen any blocks from this peer yet, we don't know where to roll back to.
            self.waiting_for_first_message = true;
            return;
        };

        let mut rolled_back = false;
        while let Some(published) = self.published_blocks.back() {
            if self
                .blocks
                .get(&published.slot)
                .is_none_or(|b| !b.was_hash_announced(id, published.hash))
            {
                self.published_blocks.pop_back();
                rolled_back = true;
                continue;
            }

            // we've found a point that's still on the chain
            if rolled_back {
                self.rolled_back_to =
                    Some(self.build_header_for_rollback(published.as_pallas_point()));
            }
            break;
        }

        // If this other chain has announced blocks which we haven't published yet,
        // queue them to be published
        let next_slot = self.published_blocks.back().map(|b| b.slot + 1).unwrap_or(peer_start.slot);
        for (slot, blocks) in self.blocks.range(next_slot..) {
            if let Some(hash) = blocks.find_announced_hash(id) {
                self.unpublished_blocks.push_back(SpecificPoint { slot: *slot, hash });
            }
        }
    }

    pub fn next_unpublished_event(&self) -> Option<ChainEvent<'_>> {
        if let Some(header) = &self.rolled_back_to {
            return Some(ChainEvent::RollBackward { header });
        }
        let block = self.unpublished_blocks.front()?;
        let slot_blocks = self.blocks.get(&block.slot)?;
        let (header, body) = slot_blocks.body(block.hash)?;
        Some(ChainEvent::RollForward { header, body })
    }

    pub fn handle_event_published(&mut self) {
        if self.rolled_back_to.take().is_some() {
            return;
        }
        if let Some(published) = self.unpublished_blocks.pop_front() {
            self.published_blocks.push_back(published);
            while self.published_blocks.len() > SECURITY_PARAMETER_K as usize {
                let Some(block) = self.published_blocks.pop_front() else {
                    break;
                };
                self.blocks.remove(&block.slot);
            }
        }
    }

    pub fn choose_points_for_find_intersect(&self) -> Vec<Point> {
        let mut iterator = self.published_blocks.iter().rev();
        let mut result = vec![];

        // send the 5 most recent points
        for _ in 0..5 {
            if let Some(point) = iterator.next() {
                result.push(point.as_pallas_point());
            }
        }

        // then 5 more points, spaced out by 10 block heights each
        let mut iterator = iterator.step_by(10);
        for _ in 0..5 {
            if let Some(point) = iterator.next() {
                result.push(point.as_pallas_point());
            }
        }

        // then 5 more points, spaced out by a total of 100 block heights each
        // (in case of an implausibly long rollback)
        let mut iterator = iterator.step_by(10);
        for _ in 0..5 {
            if let Some(point) = iterator.next() {
                result.push(point.as_pallas_point());
            }
        }

        // finally, in case of a rollback of nearly unprecedented size, fall back to the oldest point we know of
        let oldest_point = self.published_blocks.front().map(|p| p.as_pallas_point());
        if oldest_point.as_ref() != result.last()
            && let Some(point) = oldest_point
        {
            result.push(point);
        }

        result
    }

    pub fn block_announcers(&self, slot: u64, hash: BlockHash) -> Vec<PeerId> {
        match self.blocks.get(&slot) {
            Some(slot_blocks) => slot_blocks.announcers(hash),
            None => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainEvent<'a> {
    RollForward { header: &'a Header, body: &'a [u8] },
    RollBackward { header: &'a Header },
}

#[cfg(test)]
mod tests {
    use acropolis_common::Era;
    use pallas::crypto::hash::Hasher;

    use super::*;

    fn make_block(slot: u64, desc: &str) -> (Header, Vec<u8>) {
        let mut hasher = Hasher::<256>::new();
        hasher.input(&slot.to_le_bytes());
        hasher.input(desc.as_bytes());
        let hash = BlockHash::new(*hasher.finalize());
        let header = Header {
            hash,
            slot,
            number: slot,
            bytes: desc.as_bytes().to_vec(),
            era: Era::Conway,
            parent_hash: None, // Tests don't need parent hash tracking
        };
        let body = desc.as_bytes().to_vec();
        (header, body)
    }

    #[test]
    fn should_work_in_happy_path() {
        let mut state = ChainState::new();
        let peer = PeerId(0);
        state.handle_new_preferred_upstream(peer);

        let (h0, _) = make_block(0, "initial block");
        let (h1, b1) = make_block(1, "new block");

        // our peer will start with a rollback.
        state.handle_roll_backward(peer, Point::Specific(h0.slot, h0.hash.to_vec()));

        // we don't have any new events to report yet
        assert_eq!(state.next_unpublished_event(), None);

        // simulate a roll forward from our peer
        let announced = state.handle_roll_forward(peer, h1.clone());
        assert_eq!(announced, vec![peer]);

        // we don't have any new events to report yet
        assert_eq!(state.next_unpublished_event(), None);

        // report that our peer returned the body
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());

        // NOW we have a new block to report
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h1,
                body: b1.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);
    }

    #[test]
    fn should_handle_blocks_fetched_out_of_order() {
        let mut state = ChainState::new();
        let p1 = PeerId(0);
        state.handle_new_preferred_upstream(p1);

        let (h1, b1) = make_block(0, "first block");
        let (h2, b2) = make_block(1, "second block");

        // simulate a roll forward
        state.handle_roll_forward(p1, h1.clone());
        state.handle_roll_forward(p1, h2.clone());

        // we don't have any new blocks to report yet
        assert_eq!(state.next_unpublished_event(), None);

        // report that our peer returned the SECOND body first.
        state.handle_body_fetched(h2.slot, h2.hash, b2.clone());

        // without the first block, we can't use that yet.
        assert_eq!(state.next_unpublished_event(), None);

        // but once it reports the first body...
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());

        // NOW we have TWO new blocks to report
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h1,
                body: b1.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h2,
                body: b2.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);
    }

    #[test]
    fn should_handle_rollback() {
        let mut state = ChainState::new();
        let p1 = PeerId(0);
        state.handle_new_preferred_upstream(p1);

        let (h1, b1) = make_block(0, "first block");
        let (h2, b2) = make_block(1, "second block pre-rollback");
        let (h3, b3) = make_block(1, "second block post-rollback");
        let (h4, b4) = make_block(1, "third block post-rollback");

        // publish the first block
        assert_eq!(state.handle_roll_forward(p1, h1.clone()), vec![p1]);
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h1,
                body: b1.as_slice(),
            }),
        );
        state.handle_event_published();

        // publish the second block
        assert_eq!(state.handle_roll_forward(p1, h2.clone()), vec![p1]);
        state.handle_body_fetched(h2.slot, h2.hash, b2.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h2,
                body: b2.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);

        // now, roll the chain back to the first block
        state.handle_roll_backward(p1, Point::Specific(h1.slot, h1.hash.to_vec()));
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollBackward { header: &h1 }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);

        // and we should advance to the new second block
        assert_eq!(state.handle_roll_forward(p1, h3.clone()), vec![p1]);
        state.handle_body_fetched(h3.slot, h3.hash, b3.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h3,
                body: b3.as_slice(),
            }),
        );
        state.handle_event_published();

        // and the new third block should not be a rollback
        assert_eq!(state.handle_roll_forward(p1, h4.clone()), vec![p1]);
        state.handle_body_fetched(h4.slot, h4.hash, b4.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h4,
                body: b4.as_slice(),
            }),
        );
        state.handle_event_published();
    }

    #[test]
    fn should_ignore_irrelevant_block_fetch_after_rollback() {
        let mut state = ChainState::new();
        let p1 = PeerId(0);
        state.handle_new_preferred_upstream(p1);

        let (h1, b1) = make_block(0, "first block");
        let (h2a, b2a) = make_block(1, "second block pre-rollback");
        let (h3a, b3a) = make_block(2, "third block pre-rollback");
        let (h2b, b2b) = make_block(1, "second block post-rollback");
        let (h3b, b3b) = make_block(1, "third block post-rollback");

        // publish the first block
        assert_eq!(state.handle_roll_forward(p1, h1.clone()), vec![p1]);
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h1,
                body: b1.as_slice(),
            }),
        );
        state.handle_event_published();

        // publish the second block
        assert_eq!(state.handle_roll_forward(p1, h2a.clone()), vec![p1]);
        state.handle_body_fetched(h2a.slot, h2a.hash, b2a.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h2a,
                body: b2a.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);

        // roll forward to the third block, but don't receive the body yet
        assert_eq!(state.handle_roll_forward(p1, h3a.clone()), vec![p1]);

        // now, roll the chain back to the first block
        state.handle_roll_backward(p1, Point::Specific(h1.slot, h1.hash.to_vec()));
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollBackward { header: &h1 }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);

        // we should advance to the new second block
        assert_eq!(state.handle_roll_forward(p1, h2b.clone()), vec![p1]);
        state.handle_body_fetched(h2b.slot, h2b.hash, b2b.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h2b,
                body: b2b.as_slice(),
            }),
        );
        state.handle_event_published();

        // we should not take any action on receiving the original third block
        state.handle_body_fetched(h3a.slot, h3a.hash, b3a);
        assert_eq!(state.next_unpublished_event(), None);

        // and the new third block should not be a rollback
        assert_eq!(state.handle_roll_forward(p1, h3b.clone()), vec![p1]);
        state.handle_body_fetched(h3b.slot, h3b.hash, b3b.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h3b,
                body: b3b.as_slice(),
            }),
        );
        state.handle_event_published();
    }

    #[test]
    fn should_not_report_rollback_for_unpublished_portion_of_chain() {
        let mut state = ChainState::new();
        let p1 = PeerId(0);
        state.handle_new_preferred_upstream(p1);

        let (h1, b1) = make_block(0, "first block");
        let (h2, b2) = make_block(1, "second block pre-rollback");
        let (h3, b3) = make_block(1, "second block post-rollback");

        // publish the first block
        assert_eq!(state.handle_roll_forward(p1, h1.clone()), vec![p1]);
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h1,
                body: b1.as_slice(),
            }),
        );
        state.handle_event_published();

        // roll forward to the second block, but pretend the body is taking a while to download
        assert_eq!(state.handle_roll_forward(p1, h2.clone()), vec![p1]);

        // oops, we just received a rollback
        state.handle_roll_backward(p1, Point::Specific(h1.slot, h1.hash.to_vec()));

        // and THEN we got the second body
        state.handle_body_fetched(h2.slot, h2.hash, b2.clone());

        // don't publish a rollback, since we never published a roll forward.
        // also don't publish the old second block, since it isn't part of the chain
        assert_eq!(state.next_unpublished_event(), None);

        // and when we advance to the new second block, the system should not report it as a rollback
        assert_eq!(state.handle_roll_forward(p1, h3.clone()), vec![p1]);
        state.handle_body_fetched(h3.slot, h3.hash, b3.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h3,
                body: b3.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);
    }

    #[test]
    fn should_gracefully_switch_to_chain_on_fork() {
        let mut state = ChainState::new();
        let p1 = PeerId(0);
        let p2 = PeerId(1);
        state.handle_new_preferred_upstream(p1);

        let (h1, b1) = make_block(0, "first block");
        let (p1h2, p1b2) = make_block(1, "our preferred upstream's second block");
        let (p1h3, p1b3) = make_block(2, "our preferred upstream's third block");
        let (p2h2, p2b2) = make_block(1, "another upstream's second block");
        let (p2h3, p2b3) = make_block(2, "another upstream's third block");

        // publish three blocks on our current chain
        assert_eq!(state.handle_roll_forward(p1, h1.clone()), vec![p1]);
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h1,
                body: b1.as_slice(),
            }),
        );
        state.handle_event_published();

        assert_eq!(state.handle_roll_forward(p1, p1h2.clone()), vec![p1]);
        state.handle_body_fetched(p1h2.slot, p1h2.hash, p1b2.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &p1h2,
                body: p1b2.as_slice(),
            }),
        );
        state.handle_event_published();

        assert_eq!(state.handle_roll_forward(p1, p1h3.clone()), vec![p1]);
        state.handle_body_fetched(p1h3.slot, p1h3.hash, p1b3.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &p1h3,
                body: p1b3.as_slice(),
            }),
        );
        state.handle_event_published();

        // that other chain forked
        assert_eq!(state.handle_roll_forward(p2, h1.clone()), vec![p1, p2]);
        assert_eq!(state.handle_roll_forward(p2, p2h2.clone()), vec![p2]);
        state.handle_body_fetched(p2h2.slot, p2h2.hash, p2b2.clone());
        assert_eq!(state.handle_roll_forward(p2, p2h3.clone()), vec![p2]);
        state.handle_body_fetched(p2h3.slot, p2h3.hash, p2b3.clone());
        assert_eq!(state.next_unpublished_event(), None);

        // and then we decided to switch to it
        state.handle_new_preferred_upstream(p2);

        // now we should publish a rollback, followed by two blocks
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollBackward { header: &h1 }),
        );
        state.handle_event_published();
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &p2h2,
                body: p2b2.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &p2h3,
                body: p2b3.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);
    }

    #[test]
    fn should_gracefully_switch_to_new_chain_at_older_head() {
        let mut state = ChainState::new();
        let p1 = PeerId(0);
        state.handle_new_preferred_upstream(p1);

        let (h1, b1) = make_block(10, "first block");
        let (h2, b2) = make_block(11, "second block");
        let (h3, b3) = make_block(12, "third block");

        // publish three blocks on our current chain
        assert_eq!(state.handle_roll_forward(p1, h1.clone()), vec![p1]);
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h1,
                body: b1.as_slice(),
            }),
        );
        state.handle_event_published();

        assert_eq!(state.handle_roll_forward(p1, h2.clone()), vec![p1]);
        state.handle_body_fetched(h2.slot, h2.hash, b2.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h2,
                body: b2.as_slice(),
            }),
        );
        state.handle_event_published();

        assert_eq!(state.handle_roll_forward(p1, h3.clone()), vec![p1]);
        state.handle_body_fetched(h3.slot, h3.hash, b3.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h3,
                body: b3.as_slice(),
            }),
        );
        state.handle_event_published();

        // now a new peer joins, and we switch to them
        let p2 = PeerId(1);
        state.handle_new_preferred_upstream(p2);

        // We don't know enough about them to decide whether to roll back yet
        assert_eq!(state.next_unpublished_event(), None);

        // When they roll back to an earlier block, we roll back to that block.
        state.handle_roll_backward(p2, Point::Specific(h1.slot, h1.hash.to_vec()));
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollBackward { header: &h1 }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);

        // and when they roll forward to the next block, we follow
        assert_eq!(state.handle_roll_forward(p2, h2.clone()), vec![p1, p2]);
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h2,
                body: b2.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);
    }

    #[test]
    fn should_gracefully_switch_to_new_chain_at_current_head() {
        let mut state = ChainState::new();
        let p1 = PeerId(0);
        state.handle_new_preferred_upstream(p1);

        let (h1, b1) = make_block(10, "first block");
        let (h2, b2) = make_block(11, "second block");
        let (h3, b3) = make_block(12, "third block");

        // publish two blocks on our current chain
        assert_eq!(state.handle_roll_forward(p1, h1.clone()), vec![p1]);
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h1,
                body: b1.as_slice(),
            }),
        );
        state.handle_event_published();

        assert_eq!(state.handle_roll_forward(p1, h2.clone()), vec![p1]);
        state.handle_body_fetched(h2.slot, h2.hash, b2.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h2,
                body: b2.as_slice(),
            }),
        );
        state.handle_event_published();

        // now a new peer joins, and we switch to them
        let p2 = PeerId(1);
        state.handle_new_preferred_upstream(p2);

        // We don't know enough about them to decide whether to roll back yet
        assert_eq!(state.next_unpublished_event(), None);

        // They "roll back" to the point we're on, we don't react
        state.handle_roll_backward(p2, Point::Specific(h2.slot, h2.hash.to_vec()));
        assert_eq!(state.next_unpublished_event(), None);

        // They roll forward to the next point
        assert_eq!(state.handle_roll_forward(p2, h3.clone()), vec![p2]);
        state.handle_body_fetched(h3.slot, h3.hash, b3.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h3,
                body: b3.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);
    }

    #[test]
    fn should_not_drop_messages_when_switching_to_new_chain() {
        let mut state = ChainState::new();
        let p1 = PeerId(0);
        state.handle_new_preferred_upstream(p1);

        // Our initial preferred upstream is broken somehow.
        // We're not getting any messages from it, but it's not disconnecting.
        let (h1, b1) = make_block(10, "first block");
        let (h2, b2) = make_block(11, "second block");
        let (h3, b3) = make_block(12, "third block");

        // Meanwhile, another upstream is sending us blocks.
        let p2 = PeerId(1);

        assert_eq!(state.handle_roll_forward(p2, h1.clone()), vec![p2]);
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());
        assert_eq!(state.next_unpublished_event(), None);

        assert_eq!(state.handle_roll_forward(p2, h2.clone()), vec![p2]);
        state.handle_body_fetched(h2.slot, h2.hash, b2.clone());
        assert_eq!(state.next_unpublished_event(), None);

        // The initial preferred upstream finally gives up completely.
        // We switch over to one we know is wokring.
        state.handle_new_preferred_upstream(p2);

        // Immediately, we publish both blocks which it sent.
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h1,
                body: b1.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h2,
                body: b2.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);

        // And when it sends another, we publish that as well.
        assert_eq!(state.handle_roll_forward(p2, h3.clone()), vec![p2]);
        state.handle_body_fetched(h3.slot, h3.hash, b3.clone());
        assert_eq!(
            state.next_unpublished_event(),
            Some(ChainEvent::RollForward {
                header: &h3,
                body: b3.as_slice(),
            }),
        );
        state.handle_event_published();
        assert_eq!(state.next_unpublished_event(), None);
    }
}
