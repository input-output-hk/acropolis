use std::collections::{BTreeMap, VecDeque};

use acropolis_common::{BlockHash, params::SECURITY_PARAMETER_K};
use pallas::network::miniprotocols::Point;

use crate::{connection::Header, network::PeerId};

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

#[derive(Debug, Default)]
pub struct ChainState {
    pub preferred_upstream: Option<PeerId>,
    blocks: BTreeMap<u64, SlotBlockData>,
    published_blocks: VecDeque<(u64, BlockHash)>,
    unpublished_blocks: VecDeque<(u64, BlockHash)>,
    rolled_back: bool,
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
            self.unpublished_blocks.push_back((slot, hash));
        }
        self.block_announcers(slot, hash)
    }

    pub fn handle_roll_backward(&mut self, id: PeerId, point: Point) {
        let is_preferred = self.preferred_upstream == Some(id);
        match point {
            Point::Origin => {
                self.blocks.retain(|_, b| b.track_rollback(id));
                if is_preferred {
                    if !self.published_blocks.is_empty() {
                        self.rolled_back = true;
                    }
                    self.published_blocks.clear();
                    self.unpublished_blocks.clear();
                }
            }
            Point::Specific(slot, _) => {
                self.blocks.retain(|s, b| *s <= slot || b.track_rollback(id));
                if is_preferred {
                    while let Some((s, _)) = self.unpublished_blocks.back() {
                        if *s > slot {
                            self.unpublished_blocks.pop_back();
                        } else {
                            break;
                        }
                    }
                    while let Some((s, _)) = self.published_blocks.back() {
                        if *s > slot {
                            self.rolled_back = true;
                            self.published_blocks.pop_back();
                        } else {
                            break;
                        }
                    }
                }
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

        // If there are any blocks queued to be published which our preferred upstream never announced,
        // unqueue them now.
        while let Some((slot, hash)) = self.unpublished_blocks.back() {
            let Some(slot_blocks) = self.blocks.get(slot) else {
                break;
            };
            if !slot_blocks.was_hash_announced(id, *hash) {
                self.unpublished_blocks.pop_back();
            } else {
                break;
            }
        }

        // If we published any blocks which our preferred upstream never announced,
        // we'll have to publish that we rolled them back
        while let Some((slot, hash)) = self.published_blocks.back() {
            let Some(slot_blocks) = self.blocks.get(slot) else {
                break;
            };
            if !slot_blocks.was_hash_announced(id, *hash) {
                self.rolled_back = true;
                self.published_blocks.pop_back();
            } else {
                break;
            }
        }

        // If this other chain has announced blocks which we haven't published yet,
        // queue them to be published as soon as we have their bodies
        let head_slot = self.published_blocks.back().map(|(s, _)| *s);
        if let Some(slot) = head_slot {
            for (slot, blocks) in self.blocks.range(slot + 1..) {
                if let Some(hash) = blocks.find_announced_hash(id) {
                    self.unpublished_blocks.push_back((*slot, hash));
                }
            }
        }
    }

    pub fn clear_preferred_upstream(&mut self) {
        self.preferred_upstream = None;
    }

    pub fn next_unpublished_block(&self) -> Option<(&Header, &[u8], bool)> {
        let (slot, hash) = self.unpublished_blocks.front()?;
        let slot_blocks = self.blocks.get(slot)?;
        let (header, body) = slot_blocks.body(*hash)?;
        Some((header, body, self.rolled_back))
    }

    pub fn handle_block_published(&mut self) {
        if let Some(published) = self.unpublished_blocks.pop_front() {
            self.published_blocks.push_back(published);
            self.rolled_back = false;
            while self.published_blocks.len() > SECURITY_PARAMETER_K as usize {
                let Some((slot, _)) = self.published_blocks.pop_front() else {
                    break;
                };
                self.blocks.remove(&slot);
            }
        }
    }

    pub fn choose_points_for_find_intersect(&self) -> Vec<Point> {
        let mut iterator = self.published_blocks.iter().rev();
        let mut result = vec![];

        // send the 5 most recent points
        for _ in 0..5 {
            if let Some((slot, hash)) = iterator.next() {
                result.push(Point::Specific(*slot, hash.to_vec()));
            }
        }

        // then 5 more points, spaced out by 10 block heights each
        let mut iterator = iterator.step_by(10);
        for _ in 0..5 {
            if let Some((slot, hash)) = iterator.next() {
                result.push(Point::Specific(*slot, hash.to_vec()));
            }
        }

        // then 5 more points, spaced out by a total of 100 block heights each
        // (in case of an implausibly long rollback)
        let mut iterator = iterator.step_by(10);
        for _ in 0..5 {
            if let Some((slot, hash)) = iterator.next() {
                result.push(Point::Specific(*slot, hash.to_vec()));
            }
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
        };
        let body = desc.as_bytes().to_vec();
        (header, body)
    }

    #[test]
    fn should_work_in_happy_path() {
        let mut state = ChainState::new();
        let peer = PeerId(0);
        state.handle_new_preferred_upstream(peer);

        let (header, body) = make_block(0, "first block");

        // simulate a roll forward from our peer
        let announced = state.handle_roll_forward(peer, header.clone());
        assert_eq!(announced, vec![peer]);

        // we don't have any new blocks to report yet
        assert_eq!(state.next_unpublished_block(), None);

        // report that our peer returned the body
        state.handle_body_fetched(header.slot, header.hash, body.clone());

        // NOW we have a new block to report
        assert_eq!(
            state.next_unpublished_block(),
            Some((&header, body.as_slice(), false))
        );
        state.handle_block_published();
        assert_eq!(state.next_unpublished_block(), None);
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
        assert_eq!(state.next_unpublished_block(), None);

        // report that our peer returned the SECOND body first.
        state.handle_body_fetched(h2.slot, h2.hash, b2.clone());

        // without the first block, we can't use that yet.
        assert_eq!(state.next_unpublished_block(), None);

        // but once it reports the first body...
        state.handle_body_fetched(h1.slot, h1.hash, b1.clone());

        // NOW we have TWO new blocks to report
        assert_eq!(
            state.next_unpublished_block(),
            Some((&h1, b1.as_slice(), false))
        );
        state.handle_block_published();
        assert_eq!(
            state.next_unpublished_block(),
            Some((&h2, b2.as_slice(), false))
        );
        state.handle_block_published();
        assert_eq!(state.next_unpublished_block(), None);
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
            state.next_unpublished_block(),
            Some((&h1, b1.as_slice(), false))
        );
        state.handle_block_published();

        // publish the second block
        assert_eq!(state.handle_roll_forward(p1, h2.clone()), vec![p1]);
        state.handle_body_fetched(h2.slot, h2.hash, b2.clone());
        assert_eq!(
            state.next_unpublished_block(),
            Some((&h2, b2.as_slice(), false))
        );
        state.handle_block_published();
        assert_eq!(state.next_unpublished_block(), None);

        // now, roll the chain back to the first block
        state.handle_roll_backward(p1, Point::Specific(h1.slot, h1.hash.to_vec()));
        assert_eq!(state.next_unpublished_block(), None);

        // and when we advance to the new second block, the system should report it as a rollback
        assert_eq!(state.handle_roll_forward(p1, h3.clone()), vec![p1]);
        state.handle_body_fetched(h3.slot, h3.hash, b3.clone());
        assert_eq!(
            state.next_unpublished_block(),
            Some((&h3, b3.as_slice(), true))
        );
        state.handle_block_published();

        // and the new third block should not be a rollback
        assert_eq!(state.handle_roll_forward(p1, h4.clone()), vec![p1]);
        state.handle_body_fetched(h4.slot, h4.hash, b4.clone());
        assert_eq!(
            state.next_unpublished_block(),
            Some((&h4, b4.as_slice(), false))
        );
        state.handle_block_published();
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
            state.next_unpublished_block(),
            Some((&h1, b1.as_slice(), false))
        );
        state.handle_block_published();

        // roll forward to the second block, but pretend the body is taking a while to download
        assert_eq!(state.handle_roll_forward(p1, h2.clone()), vec![p1]);

        // oops, we just received a rollback
        state.handle_roll_backward(p1, Point::Specific(h1.slot, h1.hash.to_vec()));

        // and THEN we got the second body
        state.handle_body_fetched(h2.slot, h2.hash, b2.clone());

        // don't publish the old second block, since it isn't part of the chain
        assert_eq!(state.next_unpublished_block(), None);

        // and when we advance to the new second block, the system should not report it as a rollback
        assert_eq!(state.handle_roll_forward(p1, h3.clone()), vec![p1]);
        state.handle_body_fetched(h3.slot, h3.hash, b3.clone());
        assert_eq!(
            state.next_unpublished_block(),
            Some((&h3, b3.as_slice(), false))
        );
        state.handle_block_published();
        assert_eq!(state.next_unpublished_block(), None);
    }

    #[test]
    fn should_gracefully_handle_switching_chains() {
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
            state.next_unpublished_block(),
            Some((&h1, b1.as_slice(), false))
        );
        state.handle_block_published();

        assert_eq!(state.handle_roll_forward(p1, p1h2.clone()), vec![p1]);
        state.handle_body_fetched(p1h2.slot, p1h2.hash, p1b2.clone());
        assert_eq!(
            state.next_unpublished_block(),
            Some((&p1h2, p1b2.as_slice(), false))
        );
        state.handle_block_published();

        assert_eq!(state.handle_roll_forward(p1, p1h3.clone()), vec![p1]);
        state.handle_body_fetched(p1h3.slot, p1h3.hash, p1b3.clone());
        assert_eq!(
            state.next_unpublished_block(),
            Some((&p1h3, p1b3.as_slice(), false))
        );
        state.handle_block_published();

        // that other chain forked
        assert_eq!(state.handle_roll_forward(p2, h1.clone()), vec![p1, p2]);
        assert_eq!(state.handle_roll_forward(p2, p2h2.clone()), vec![p2]);
        state.handle_body_fetched(p2h2.slot, p2h2.hash, p2b2.clone());
        assert_eq!(state.handle_roll_forward(p2, p2h3.clone()), vec![p2]);
        state.handle_body_fetched(p2h3.slot, p2h3.hash, p2b3.clone());
        assert_eq!(state.next_unpublished_block(), None);

        // and then we decided to switch to it
        state.handle_new_preferred_upstream(p2);

        // now we should publish two blocks, and the first should be marked as "rollback"
        assert_eq!(
            state.next_unpublished_block(),
            Some((&p2h2, p2b2.as_slice(), true))
        );
        state.handle_block_published();
        assert_eq!(
            state.next_unpublished_block(),
            Some((&p2h3, p2b3.as_slice(), false))
        );
        state.handle_block_published();
        assert_eq!(state.next_unpublished_block(), None);
    }
}
