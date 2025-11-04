use std::collections::{BTreeMap, VecDeque};

use acropolis_common::{BlockHash, params::SECURITY_PARAMETER_K};
use pallas::network::miniprotocols::Point;

use crate::{connection::Header, network::PeerId};

struct BlockData {
    header: Header,
    announced_by: Vec<PeerId>,
    body: Option<Vec<u8>>,
}

#[derive(Default)]
struct SlotBlockData {
    blocks: Vec<BlockData>,
}
impl SlotBlockData {
    fn track_announcement(&mut self, id: PeerId, header: Header) {
        if let Some(block) = self.blocks.iter_mut().find(|b| b.header.hash == header.hash) {
            block.announced_by.push(id);
        } else {
            self.blocks.push(BlockData { header, announced_by: vec![id], body: None });
        }
    }

    fn announced(&self, id: PeerId, hash: BlockHash) -> bool {
        self.blocks.iter().any(|b| b.header.hash == hash && b.announced_by.contains(&id))
    }

    fn announcers(&self, hash: BlockHash) -> Vec<PeerId> {
        match self.blocks.iter().find(|b| b.header.hash == hash) {
            Some(b) => b.announced_by.clone(),
            None => vec![]
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

#[derive(Default)]
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
            self.block_announcers(slot, hash)
        } else {
            vec![]
        }
    }

    pub fn handle_roll_backward(&mut self, id: PeerId, point: Point) -> bool {
        let is_preferred = self.preferred_upstream == Some(id);
        if !is_preferred {
            return false;
        }
        match point {
            Point::Origin => {
                self.rolled_back = !self.published_blocks.is_empty();
                self.published_blocks.clear();
                self.unpublished_blocks.clear();
                self.rolled_back
            }
            Point::Specific(slot, _) => {
                while let Some((s, _)) = self.unpublished_blocks.back() {
                    if *s > slot {
                        self.unpublished_blocks.pop_back();
                    } else {
                        break;
                    }
                }
                self.rolled_back = false;
                while let Some((s, _)) = self.published_blocks.back() {
                    if *s > slot {
                        self.rolled_back = true;
                        self.published_blocks.pop_back();
                    } else {
                        break;
                    }
                }
                self.rolled_back
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
        while let Some((slot, hash)) = self.unpublished_blocks.back() {
            let Some(slot_blocks) = self.blocks.get(slot) else {
                break;
            };
            if !slot_blocks.announced(id, *hash) {
                self.unpublished_blocks.pop_back();
            }
        }
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
                let Some((slot, _)) = self.published_blocks.pop_back() else {
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
            } else {
                result.push(Point::Origin);
                return result;
            }
        }

        // then 5 more points, spaced out by 10 block heights each
        let mut iterator = iterator.step_by(10);
        for _ in 0..5 {
            if let Some((slot, hash)) = iterator.next() {
                result.push(Point::Specific(*slot, hash.to_vec()));
            } else {
                result.push(Point::Origin);
                return result;
            }
        }

        // then 5 more points, spaced out by a total of 100 block heights each
        // (in case of an implausibly long rollback)
        let mut iterator = iterator.step_by(10);
        for _ in 0..5 {
            if let Some((slot, hash)) = iterator.next() {
                result.push(Point::Specific(*slot, hash.to_vec()));
            } else {
                result.push(Point::Origin);
                return result;
            }
        }

        result
    }

    pub fn block_announcers(&self, slot: u64, hash: BlockHash) -> Vec<PeerId> {
        match self.blocks.get(&slot) {
            Some(slot_blocks) => slot_blocks.announcers(hash),
            None => vec![]
        }
    }
}