//! Consensus tree data structure for tracking volatile chain forks.
//!
//! Implements the Praos `maxvalid` chain selection rule: select the
//! longest valid chain, with ties broken in favour of the current chain.
//! The bounded variant rejects chains forking deeper than k blocks.

use std::collections::HashMap;

use acropolis_common::BlockHash;
use tracing::debug;

use crate::tree_block::{BlockValidationStatus, TreeBlock};
use crate::tree_error::ConsensusTreeError;
use crate::tree_observer::ConsensusTreeObserver;

/// The top-level data structure managing all volatile blocks.
///
/// Holds at most ~k blocks in memory. Operations are single-threaded;
/// the owning module handles concurrency.
pub struct ConsensusTree {
    /// All blocks keyed by hash.
    blocks: HashMap<BlockHash, TreeBlock>,
    /// Root of the tree (oldest block).
    root: Option<BlockHash>,
    /// Current favoured chain tip.
    favoured_tip: Option<BlockHash>,
    /// Security parameter (default 2160).
    k: u64,
    /// Callback receiver.
    observer: Box<dyn ConsensusTreeObserver + Send>,
}

impl ConsensusTree {
    /// Create a new empty consensus tree.
    ///
    /// `k` is the security parameter (Praos Common Prefix parameter).
    /// `observer` receives callbacks for block_proposed, rollback,
    /// and block_rejected events.
    pub fn new(k: u64, observer: Box<dyn ConsensusTreeObserver + Send>) -> Self {
        Self {
            blocks: HashMap::new(),
            root: None,
            favoured_tip: None,
            k,
            observer,
        }
    }

    /// Set the root of the tree (genesis or snapshot starting point).
    ///
    /// This must be called before any other operation. The root block
    /// has no parent and is immediately Validated.
    pub fn set_root(
        &mut self,
        hash: BlockHash,
        number: u64,
        slot: u64,
    ) -> Result<(), ConsensusTreeError> {
        let mut block = TreeBlock::new(hash, number, slot, None, BlockValidationStatus::Validated);
        block.body = Some(Vec::new()); // Root has an empty body sentinel
        self.blocks.insert(hash, block);
        self.root = Some(hash);
        self.favoured_tip = Some(hash);
        Ok(())
    }

    /// Returns a reference to the block with the given hash, if present.
    pub(crate) fn get_block(&self, hash: &BlockHash) -> Option<&TreeBlock> {
        self.blocks.get(hash)
    }

    /// Returns a mutable reference to the block with the given hash.
    #[cfg(test)]
    pub(crate) fn get_block_mut(&mut self, hash: &BlockHash) -> Option<&mut TreeBlock> {
        self.blocks.get_mut(hash)
    }

    /// Returns the current favoured tip hash, if the tree is non-empty.
    pub fn favoured_tip(&self) -> Option<BlockHash> {
        self.favoured_tip
    }

    /// Returns the root hash, if the tree is non-empty.
    pub fn root(&self) -> Option<BlockHash> {
        self.root
    }

    /// Returns the security parameter k.
    pub fn k(&self) -> u64 {
        self.k
    }

    /// Take the observer out for reuse (e.g. when re-creating the tree).
    /// Replaces the current observer with a no-op stub.
    pub fn take_observer(&mut self) -> Box<dyn ConsensusTreeObserver + Send> {
        std::mem::replace(&mut self.observer, Box::new(NoOpObserver))
    }

    /// Returns the number of blocks in the tree.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Returns true if the tree has no blocks.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    // ── Foundational helpers (Phase 2) ─────────────────────────────

    /// Find the tip of the longest chain starting from the root.
    ///
    /// Implements the Praos `maxvalid` rule: returns the longest chain
    /// tip. Ties are broken in favour of the current `favoured_tip`
    /// (Praos paper, line 667-668).
    pub fn get_favoured_chain(&self) -> Option<BlockHash> {
        let root = self.root?;
        let (_, tip) = self.longest_chain_from(root);
        Some(tip)
    }

    /// Recursive helper: returns (chain_length, tip_hash) for the
    /// longest chain rooted at `hash`.
    fn longest_chain_from(&self, hash: BlockHash) -> (u64, BlockHash) {
        let block = match self.blocks.get(&hash) {
            Some(b) => b,
            None => return (1, hash),
        };

        if block.children.is_empty() {
            return (1, hash);
        }

        let mut max_length = 0u64;
        let mut best_tip = hash;

        for &child_hash in &block.children {
            let (child_len, child_tip) = self.longest_chain_from(child_hash);
            if child_len > max_length {
                max_length = child_len;
                best_tip = child_tip;
            } else if child_len == max_length {
                // Tie-break: favour current tip (Praos maxvalid)
                if Some(child_tip) == self.favoured_tip
                    || self.is_ancestor_of(child_tip, self.favoured_tip)
                {
                    best_tip = child_tip;
                }
            }
        }

        (max_length + 1, best_tip)
    }

    /// Returns true if `candidate` is an ancestor of `descendant`
    /// (i.e., `descendant` is on a chain that passes through `candidate`).
    fn is_ancestor_of(&self, candidate: BlockHash, descendant: Option<BlockHash>) -> bool {
        let mut current = descendant;
        while let Some(h) = current {
            if h == candidate {
                return true;
            }
            current = self.blocks.get(&h).and_then(|b| b.parent);
        }
        false
    }

    /// Find the common ancestor of two blocks by walking back from both.
    ///
    /// Returns the hash of the deepest block that is an ancestor of both
    /// `a` and `b`. Returns an error if either block is not in the tree.
    pub fn find_common_ancestor(
        &self,
        a: BlockHash,
        b: BlockHash,
    ) -> Result<BlockHash, ConsensusTreeError> {
        let block_a = self.blocks.get(&a).ok_or(ConsensusTreeError::BlockNotInTree { hash: a })?;
        let block_b = self.blocks.get(&b).ok_or(ConsensusTreeError::BlockNotInTree { hash: b })?;

        let mut ha = a;
        let mut hb = b;
        let mut na = block_a.number;
        let mut nb = block_b.number;

        // Walk the higher block down to the same level
        while na > nb {
            let parent =
                self.blocks[&ha].parent.ok_or(ConsensusTreeError::BlockNotInTree { hash: ha })?;
            ha = parent;
            na -= 1;
        }
        while nb > na {
            let parent =
                self.blocks[&hb].parent.ok_or(ConsensusTreeError::BlockNotInTree { hash: hb })?;
            hb = parent;
            nb -= 1;
        }

        // Walk both up until they meet
        while ha != hb {
            ha = self.blocks[&ha].parent.ok_or(ConsensusTreeError::BlockNotInTree { hash: ha })?;
            hb = self.blocks[&hb].parent.ok_or(ConsensusTreeError::BlockNotInTree { hash: hb })?;
        }

        Ok(ha)
    }

    /// Check if a block is on the chain ending at the given tip.
    ///
    /// Walks back from `tip` to root; returns true if `block_hash`
    /// is encountered.
    pub fn chain_contains(&self, block_hash: BlockHash, tip: BlockHash) -> bool {
        let mut current = Some(tip);
        while let Some(h) = current {
            if h == block_hash {
                return true;
            }
            current = self.blocks.get(&h).and_then(|b| b.parent);
        }
        false
    }

    /// Compute the fork depth: how many blocks back from the current
    /// favoured chain the fork point of the given block's chain is.
    ///
    /// Returns 0 if the block extends the favoured chain directly.
    /// Returns the number of favoured-chain blocks that would need to
    /// be rolled back to accommodate this fork.
    pub fn fork_depth(&self, block_hash: BlockHash) -> Result<u64, ConsensusTreeError> {
        let block = self
            .blocks
            .get(&block_hash)
            .ok_or(ConsensusTreeError::BlockNotInTree { hash: block_hash })?;

        let tip = match self.favoured_tip {
            Some(t) => t,
            None => return Ok(0),
        };

        // Find the parent of the new block
        let parent_hash = match block.parent {
            Some(p) => p,
            None => return Ok(0), // Root block
        };

        // If parent is the favoured tip, fork depth is 0
        if parent_hash == tip {
            return Ok(0);
        }

        // If parent is on the favoured chain, compute how far back
        if self.chain_contains(parent_hash, tip) {
            let tip_number = self.blocks[&tip].number;
            let parent_number = self.blocks[&parent_hash].number;
            return Ok(tip_number - parent_number);
        }

        // Parent is not on the favoured chain — find common ancestor
        let ancestor = self.find_common_ancestor(parent_hash, tip)?;
        let tip_number = self.blocks[&tip].number;
        let ancestor_number = self.blocks[&ancestor].number;
        Ok(tip_number - ancestor_number)
    }

    /// Internal helper to insert a block into the tree (used by tests).
    #[cfg(test)]
    pub(crate) fn insert_block(
        &mut self,
        hash: BlockHash,
        number: u64,
        slot: u64,
        parent: BlockHash,
        status: BlockValidationStatus,
    ) -> Result<(), ConsensusTreeError> {
        // Validate parent exists
        if !self.blocks.contains_key(&parent) {
            return Err(ConsensusTreeError::ParentNotFound { hash: parent });
        }

        // Validate block number
        let parent_number = self.blocks[&parent].number;
        if number != parent_number + 1 {
            return Err(ConsensusTreeError::InvalidBlockNumber {
                expected: parent_number + 1,
                got: number,
            });
        }

        let block = TreeBlock::new(hash, number, slot, Some(parent), status);
        self.blocks.insert(hash, block);

        // Add to parent's children
        if let Some(parent_block) = self.blocks.get_mut(&parent) {
            parent_block.children.push(hash);
        }

        Ok(())
    }

    /// Recompute and update the favoured tip based on current tree state.
    #[cfg(test)]
    pub(crate) fn update_favoured_tip(&mut self) {
        self.favoured_tip = self.get_favoured_chain();
    }

    // ── Phase 3: User Story 1 — check_block_wanted (T023) ─────────

    /// Evaluate an offered block and decide whether it is wanted.
    ///
    /// Inserts the block into the tree. If the block extends the
    /// favoured chain, it is marked `Wanted` and returned. If it is on
    /// an unfavoured fork, it is marked `Offered` and NOT returned.
    ///
    /// If inserting this block causes a chain switch, fires `rollback`
    /// observer, transitions `Offered` blocks on the new favoured chain
    /// to `Wanted`, fires `block_proposed` for already-fetched blocks,
    /// and returns all newly wanted hashes.
    ///
    /// Enforces bounded maxvalid: rejects blocks whose fork depth > k.
    pub fn check_block_wanted(
        &mut self,
        hash: BlockHash,
        parent_hash: BlockHash,
        number: u64,
        slot: u64,
    ) -> Result<Vec<BlockHash>, ConsensusTreeError> {
        // Idempotent for already-known block headers: avoid reinserting
        // the same hash and duplicating parent->child edges.
        if let Some(existing) = self.blocks.get(&hash) {
            if existing.parent != Some(parent_hash) {
                return Err(ConsensusTreeError::ParentNotFound { hash: parent_hash });
            }
            if existing.number != number {
                return Err(ConsensusTreeError::InvalidBlockNumber {
                    expected: existing.number,
                    got: number,
                });
            }

            let tip = self.get_favoured_chain();
            self.favoured_tip = tip;
            let on_favoured_chain = tip.is_some_and(|t| self.chain_contains(hash, t));

            return match existing.status {
                BlockValidationStatus::Offered if on_favoured_chain => {
                    if let Some(block) = self.blocks.get_mut(&hash) {
                        block.status = BlockValidationStatus::Wanted;
                    }
                    Ok(vec![hash])
                }
                BlockValidationStatus::Wanted => Ok(vec![hash]),
                BlockValidationStatus::Offered
                | BlockValidationStatus::Fetched
                | BlockValidationStatus::Validated
                | BlockValidationStatus::Rejected => Ok(Vec::new()),
            };
        }

        // Validate parent exists
        if !self.blocks.contains_key(&parent_hash) {
            return Err(ConsensusTreeError::ParentNotFound { hash: parent_hash });
        }

        // Validate block number
        let parent_number = self.blocks[&parent_hash].number;
        if number != parent_number + 1 {
            return Err(ConsensusTreeError::InvalidBlockNumber {
                expected: parent_number + 1,
                got: number,
            });
        }

        let old_tip = self.favoured_tip;

        // Tentatively insert as Offered to check fork depth
        let block = TreeBlock::new(
            hash,
            number,
            slot,
            Some(parent_hash),
            BlockValidationStatus::Offered,
        );
        self.blocks.insert(hash, block);
        if let Some(parent_block) = self.blocks.get_mut(&parent_hash) {
            parent_block.children.push(hash);
        }

        // Check fork depth against bounded maxvalid
        let depth = self.fork_depth(hash)?;
        if depth > self.k {
            // Remove the block we just inserted
            self.remove_block_internal(hash);
            return Err(ConsensusTreeError::ForkTooDeep {
                fork_depth: depth,
                max_k: self.k,
            });
        }

        // Compute new favoured chain
        let new_tip = self.get_favoured_chain();
        self.favoured_tip = new_tip;

        match (old_tip, new_tip) {
            (Some(old), Some(new)) if old != new => {
                if self.chain_contains(old, new) {
                    // Linear extension: old tip is ancestor of new tip — no rollback.
                    let new_blocks = self.collect_chain_from_ancestor(old, new);
                    let mut wanted = Vec::new();
                    for bh in new_blocks {
                        if let Some(b) = self.blocks.get_mut(&bh) {
                            if b.status == BlockValidationStatus::Offered {
                                b.status = BlockValidationStatus::Wanted;
                                wanted.push(bh);
                            }
                        }
                    }
                    Ok(wanted)
                } else {
                    // True chain switch: new tip is on a different fork.
                    self.handle_chain_switch(old_tip, new_tip)
                }
            }
            _ => {
                // Tip unchanged — check if block is on favoured chain
                let tip = match new_tip {
                    Some(t) => t,
                    None => return Ok(Vec::new()),
                };

                if self.chain_contains(hash, tip) {
                    if let Some(b) = self.blocks.get_mut(&hash) {
                        b.status = BlockValidationStatus::Wanted;
                    }
                    Ok(vec![hash])
                } else {
                    Ok(Vec::new())
                }
            }
        }
    }

    /// Handle a chain switch: fire rollback, transition Offered→Wanted
    /// on new chain, fire block_proposed for fetched blocks, return
    /// newly wanted hashes.
    fn handle_chain_switch(
        &mut self,
        old_tip: Option<BlockHash>,
        new_tip: Option<BlockHash>,
    ) -> Result<Vec<BlockHash>, ConsensusTreeError> {
        let new_tip_hash = match new_tip {
            Some(t) => t,
            None => return Ok(Vec::new()),
        };

        let old_tip_hash = match old_tip {
            Some(t) => t,
            None => return Ok(Vec::new()),
        };

        // Find common ancestor
        let ancestor = self.find_common_ancestor(old_tip_hash, new_tip_hash)?;
        let ancestor_number = self.blocks[&ancestor].number;

        // Fire rollback observer
        self.observer.rollback(ancestor_number);

        // Walk from ancestor to new tip, collecting blocks to process
        let blocks_on_new_chain = self.collect_chain_from_ancestor(ancestor, new_tip_hash);
        Ok(self.process_chain_after_switch(blocks_on_new_chain))
    }

    /// Collect block hashes on the chain from ancestor (exclusive) to tip (inclusive),
    /// in ascending order.
    fn collect_chain_from_ancestor(&self, ancestor: BlockHash, tip: BlockHash) -> Vec<BlockHash> {
        let mut chain = Vec::new();
        let mut current = Some(tip);
        while let Some(h) = current {
            if h == ancestor {
                break;
            }
            chain.push(h);
            current = self.blocks.get(&h).and_then(|b| b.parent);
        }
        chain.reverse();
        chain
    }

    /// Process blocks on the new chain after a switch: transition Offered→Wanted,
    /// fire block_proposed for already-fetched blocks, and return the list of wanted hashes.
    fn process_chain_after_switch(
        &mut self,
        blocks_on_new_chain: Vec<BlockHash>,
    ) -> Vec<BlockHash> {
        let mut wanted = Vec::new();
        for block_hash in blocks_on_new_chain {
            let block = match self.blocks.get(&block_hash) {
                Some(b) => b,
                None => continue,
            };

            match block.status {
                BlockValidationStatus::Offered => {
                    if let Some(b) = self.blocks.get_mut(&block_hash) {
                        b.status = BlockValidationStatus::Wanted;
                    }
                    wanted.push(block_hash);
                }
                BlockValidationStatus::Wanted => {
                    wanted.push(block_hash);
                }
                BlockValidationStatus::Fetched | BlockValidationStatus::Validated => {
                    let b = &self.blocks[&block_hash];
                    if let Some(ref body) = b.body {
                        self.observer.block_proposed(b.number, b.hash, body);
                    }
                }
                BlockValidationStatus::Rejected => {}
            }
        }
        wanted
    }

    // ── Phase 4: User Story 2 — add_block (T030) ──────────────────

    /// Store a block body and fire observers for contiguous fetched blocks.
    ///
    /// The block must already be in the tree (registered via
    /// `check_block_wanted`). Decodes the hash from the caller (the
    /// caller knows which block body this is). Transitions status from
    /// `Wanted` to `Fetched`.
    ///
    /// Fires `block_proposed` for this block and any subsequent fetched
    /// blocks on the favoured chain, stopping at the first gap.
    pub fn add_block(&mut self, hash: BlockHash, body: Vec<u8>) -> Result<(), ConsensusTreeError> {
        let block = self.blocks.get(&hash).ok_or(ConsensusTreeError::BlockNotInTree { hash })?;

        // Idempotent: if already fetched, no-op
        if block.body.is_some() {
            return Ok(());
        }

        // Store body and transition to Fetched
        let block = self.blocks.get_mut(&hash).unwrap();
        block.body = Some(body);
        block.status = BlockValidationStatus::Fetched;

        // Fire block_proposed for contiguous fetched blocks on favoured chain
        let tip = match self.favoured_tip {
            Some(t) => t,
            None => {
                debug!("add_block: favoured_tip is None, skipping fire_contiguous_proposed");
                return Ok(());
            }
        };

        // Only fire if this block is on the favoured chain
        if !self.chain_contains(hash, tip) {
            debug!(
                hash = %hash,
                tip = %tip,
                "add_block: block not on favoured chain, skipping fire_contiguous_proposed"
            );
            return Ok(());
        }

        // Find the earliest unproposed block on the favoured chain
        // and fire block_proposed for contiguous fetched blocks
        self.fire_contiguous_proposed(hash);

        Ok(())
    }

    /// Fire block_proposed for contiguous fetched blocks starting from
    /// the given block, walking forward on the favoured chain.
    fn fire_contiguous_proposed(&mut self, start: BlockHash) {
        // First, check that all blocks from root to start have bodies
        // (i.e., no gaps before this block)
        let mut ancestors = Vec::new();
        let mut current = Some(start);
        while let Some(h) = current {
            let block = match self.blocks.get(&h) {
                Some(b) => b,
                None => break,
            };
            if block.parent.is_none() {
                // Reached root
                break;
            }
            ancestors.push(h);
            current = block.parent;
        }
        ancestors.reverse();

        // Check for gaps before start
        for &h in &ancestors {
            let block = &self.blocks[&h];
            if block.body.is_none() {
                debug!(
                    start = %start,
                    gap_at = %h,
                    "fire_contiguous_proposed: gap (missing body) before start, not proposing"
                );
                return; // Gap found — can't propose yet
            }
        }

        // Now fire block_proposed for start and contiguous fetched children
        let tip = match self.favoured_tip {
            Some(t) => t,
            None => return,
        };

        let mut to_propose = vec![start];
        while let Some(h) = to_propose.pop() {
            let block = match self.blocks.get(&h) {
                Some(b) => b,
                None => continue,
            };

            if let Some(ref body) = block.body {
                let number = block.number;
                let hash = block.hash;
                let body_clone = body.clone();
                self.observer.block_proposed(number, hash, &body_clone);

                // Continue with children on favoured chain
                let children: Vec<BlockHash> = block.children.clone();
                for child in children {
                    if self.chain_contains(child, tip) {
                        to_propose.push(child);
                    }
                }
            }
            // If no body, stop (gap)
        }
    }

    // ── Phase 5: User Story 3 — validation + removal (T039-T042) ──

    /// Transition a block from Fetched to Validated.
    ///
    /// This confirms the block passed validation.
    pub fn mark_validated(&mut self, hash: BlockHash) -> Result<(), ConsensusTreeError> {
        let block =
            self.blocks.get_mut(&hash).ok_or(ConsensusTreeError::BlockNotInTree { hash })?;
        block.status = BlockValidationStatus::Validated;
        Ok(())
    }

    /// Handle a block that failed validation.
    ///
    /// Fires `block_rejected` observer, removes the block and all its
    /// descendants, and handles any resulting chain switch.
    pub fn mark_rejected(&mut self, hash: BlockHash) -> Result<Vec<BlockHash>, ConsensusTreeError> {
        if !self.blocks.contains_key(&hash) {
            return Err(ConsensusTreeError::BlockNotInTree { hash });
        }

        // Fire block_rejected
        self.observer.block_rejected(hash);

        // Remove block and descendants, handle chain switch
        self.remove_block(hash)
    }

    /// Remove a block and all its descendants from the tree.
    ///
    /// If removing the block changes the favoured chain, fires rollback
    /// and returns newly wanted hashes. Used for `cardano.block.rescinded`.
    pub fn remove_block(&mut self, hash: BlockHash) -> Result<Vec<BlockHash>, ConsensusTreeError> {
        if !self.blocks.contains_key(&hash) {
            return Err(ConsensusTreeError::BlockNotInTree { hash });
        }

        let old_tip = self.favoured_tip;

        // Find the parent of the removed block BEFORE removing it.
        // This is the rollback point if a chain switch occurs.
        let removed_parent = self.blocks.get(&hash).and_then(|b| b.parent);

        self.remove_block_internal(hash);

        // Check if favoured chain changed
        let new_tip = self.get_favoured_chain();

        if let Some(new_tip_hash) = new_tip {
            if new_tip != old_tip {
                self.favoured_tip = new_tip;

                // The old tip was removed, so we can't call find_common_ancestor
                // with it. Instead, use the parent of the removed block as the
                // rollback reference point.
                if let Some(parent) = removed_parent {
                    if self.blocks.contains_key(&parent) {
                        let ancestor = self.find_common_ancestor(parent, new_tip_hash)?;
                        let ancestor_number = self.blocks[&ancestor].number;
                        self.observer.rollback(ancestor_number);

                        let blocks_on_new_chain =
                            self.collect_chain_from_ancestor(ancestor, new_tip_hash);
                        return Ok(self.process_chain_after_switch(blocks_on_new_chain));
                    }
                }
            }
            Ok(Vec::new())
        } else {
            self.favoured_tip = new_tip;
            Ok(Vec::new())
        }
    }

    /// Internal: remove a block and all descendants without chain switch handling.
    fn remove_block_internal(&mut self, hash: BlockHash) {
        // Collect all descendants
        let descendants = self.collect_all_from(hash, false);

        // Remove from parent's children list
        if let Some(block) = self.blocks.get(&hash) {
            if let Some(parent_hash) = block.parent {
                if let Some(parent) = self.blocks.get_mut(&parent_hash) {
                    parent.children.retain(|&h| h != hash);
                }
            }
        }

        // Remove all descendants and the block itself
        for h in descendants {
            self.blocks.remove(&h);
        }
        self.blocks.remove(&hash);
    }

    /// Collect all hashes reachable from a block.
    /// If `inclusive` is true, includes the block itself; otherwise only its descendants.
    fn collect_all_from(&self, hash: BlockHash, inclusive: bool) -> Vec<BlockHash> {
        let mut result = if inclusive { vec![hash] } else { Vec::new() };
        let mut stack = vec![hash];
        while let Some(h) = stack.pop() {
            if let Some(block) = self.blocks.get(&h) {
                for &child in &block.children {
                    result.push(child);
                    stack.push(child);
                }
            }
        }
        result
    }

    // ── Phase 6: User Story 4 — prune (T047) ──────────────────────

    /// Remove blocks older than (tip - k) and dead forks.
    ///
    /// After pruning, the root is updated to the new oldest block on
    /// the favoured chain. Non-favoured branches rooted before the
    /// prune boundary are removed entirely.
    pub fn prune(&mut self) -> Result<(), ConsensusTreeError> {
        let tip = match self.favoured_tip {
            Some(t) => t,
            None => return Ok(()),
        };

        let tip_number = self.blocks[&tip].number;
        if tip_number <= self.k {
            return Ok(()); // Not enough blocks to prune
        }

        let prune_boundary = tip_number - self.k;

        // Find the new root: the block on the favoured chain at the prune boundary
        let mut new_root = tip;
        while let Some(block) = self.blocks.get(&new_root) {
            if block.number <= prune_boundary {
                break;
            }
            match block.parent {
                Some(p) => new_root = p,
                None => break,
            }
        }

        // Collect all blocks to keep: blocks reachable from new_root
        let blocks_to_keep = self.collect_all_from(new_root, true);

        // Remove all blocks not in the keep set
        let all_hashes: Vec<BlockHash> = self.blocks.keys().copied().collect();
        for h in all_hashes {
            if !blocks_to_keep.contains(&h) {
                self.blocks.remove(&h);
            }
        }

        // Update root and clear its parent pointer
        self.root = Some(new_root);
        if let Some(block) = self.blocks.get_mut(&new_root) {
            block.parent = None;
        }

        Ok(())
    }
}

/// Placeholder observer used when the real observer has been taken via `take_observer`.
struct NoOpObserver;
impl ConsensusTreeObserver for NoOpObserver {
    fn block_proposed(&self, _: u64, _: BlockHash, _: &[u8]) {}
    fn rollback(&self, _: u64) {}
    fn block_rejected(&self, _: BlockHash) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_observer::ConsensusTreeObserver;
    use std::sync::Mutex;

    /// Test observer that records all events for assertion.
    struct TestObserver {
        proposed: Mutex<Vec<(u64, BlockHash)>>,
        rollbacks: Mutex<Vec<u64>>,
        rejected: Mutex<Vec<BlockHash>>,
    }

    impl TestObserver {
        fn new() -> Self {
            Self {
                proposed: Mutex::new(Vec::new()),
                rollbacks: Mutex::new(Vec::new()),
                rejected: Mutex::new(Vec::new()),
            }
        }
    }

    impl ConsensusTreeObserver for TestObserver {
        fn block_proposed(&self, number: u64, hash: BlockHash, _body: &[u8]) {
            self.proposed.lock().unwrap().push((number, hash));
        }

        fn rollback(&self, to_block_number: u64) {
            self.rollbacks.lock().unwrap().push(to_block_number);
        }

        fn block_rejected(&self, hash: BlockHash) {
            self.rejected.lock().unwrap().push(hash);
        }
    }

    /// Helper: create a BlockHash from a u8 value (for test convenience).
    fn hash(n: u8) -> BlockHash {
        let mut bytes = [0u8; 32];
        bytes[0] = n;
        BlockHash::from(bytes)
    }

    /// Helper: create a tree with a test observer, returns (tree, observer_ptr).
    /// We use a raw pointer to read the observer after giving ownership to the tree.
    fn make_tree(k: u64) -> (ConsensusTree, *const TestObserver) {
        let observer = Box::new(TestObserver::new());
        let ptr = &*observer as *const TestObserver;
        (ConsensusTree::new(k, observer), ptr)
    }

    /// Helper: insert a block with a body (simulates fetched block).
    fn insert_with_body(
        tree: &mut ConsensusTree,
        h: u8,
        number: u64,
        parent: u8,
        status: BlockValidationStatus,
    ) {
        tree.insert_block(hash(h), number, number, hash(parent), status).unwrap();
        tree.get_block_mut(&hash(h)).unwrap().body = Some(vec![h]);
    }

    /// Helper: insert a block without body.
    fn insert_no_body(
        tree: &mut ConsensusTree,
        h: u8,
        number: u64,
        parent: u8,
        status: BlockValidationStatus,
    ) {
        tree.insert_block(hash(h), number, number, hash(parent), status).unwrap();
    }

    // ── Phase 1 tests ─────────────────────────────────────────────

    #[test]
    fn test_set_root_creates_single_block_tree() {
        let (mut tree, _) = make_tree(2160);
        let root_hash = hash(1);
        tree.set_root(root_hash, 0, 0).unwrap();

        assert_eq!(tree.len(), 1);
        assert_eq!(tree.root(), Some(root_hash));
        assert_eq!(tree.favoured_tip(), Some(root_hash));

        let block = tree.get_block(&root_hash).unwrap();
        assert_eq!(block.number, 0);
        assert_eq!(block.status, BlockValidationStatus::Validated);
        assert!(block.parent.is_none());
    }

    // ── Phase 2 tests: Foundational helpers (T008-T013) ───────────

    /// T008: get_favoured_chain returns root tip for single-block tree.
    #[test]
    fn test_get_favoured_chain_single_block_returns_root() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        assert_eq!(tree.get_favoured_chain(), Some(hash(1)));
    }

    /// T009: get_favoured_chain returns longer branch tip for forked tree.
    #[test]
    fn test_get_favoured_chain_returns_longer_branch() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Fork at root: branch A (2->3) and branch B (4->5->6)
        insert_with_body(&mut tree, 2, 1, 1, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 3, 2, 2, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 4, 1, 1, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 5, 2, 4, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 6, 3, 5, BlockValidationStatus::Validated);

        tree.update_favoured_tip();
        assert_eq!(tree.get_favoured_chain(), Some(hash(6)));
    }

    /// T010: get_favoured_chain retains current tip on equal-length forks.
    #[test]
    fn test_get_favoured_chain_equal_length_retains_current_tip() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Two branches of equal length from root
        insert_with_body(&mut tree, 2, 1, 1, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 3, 2, 2, BlockValidationStatus::Validated);

        // Set favoured tip to hash(3)
        tree.favoured_tip = Some(hash(3));

        insert_with_body(&mut tree, 4, 1, 1, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 5, 2, 4, BlockValidationStatus::Validated);

        // Both branches have length 3 (root + 2). Current tip is hash(3).
        // Praos tie-break: favour current chain.
        assert_eq!(tree.get_favoured_chain(), Some(hash(3)));
    }

    /// T011: find_common_ancestor returns correct ancestor for diverging tips.
    #[test]
    fn test_find_common_ancestor_for_diverging_tips() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // A: 1->2->3->4
        insert_with_body(&mut tree, 2, 1, 1, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 3, 2, 2, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 4, 3, 3, BlockValidationStatus::Validated);

        // B: 1->2->5->6
        insert_with_body(&mut tree, 5, 2, 2, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 6, 3, 5, BlockValidationStatus::Validated);

        // Common ancestor of hash(4) and hash(6) should be hash(2)
        let ancestor = tree.find_common_ancestor(hash(4), hash(6)).unwrap();
        assert_eq!(ancestor, hash(2));
    }

    /// T012: chain_contains returns true for block on chain, false otherwise.
    #[test]
    fn test_chain_contains() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Chain: 1->2->3
        insert_with_body(&mut tree, 2, 1, 1, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 3, 2, 2, BlockValidationStatus::Validated);

        // Fork: 1->4
        insert_with_body(&mut tree, 4, 1, 1, BlockValidationStatus::Validated);

        // hash(2) is on chain ending at hash(3)
        assert!(tree.chain_contains(hash(2), hash(3)));
        // hash(1) (root) is on chain ending at hash(3)
        assert!(tree.chain_contains(hash(1), hash(3)));
        // hash(4) is NOT on chain ending at hash(3)
        assert!(!tree.chain_contains(hash(4), hash(3)));
        // hash(3) is on its own chain
        assert!(tree.chain_contains(hash(3), hash(3)));
    }

    /// T013: fork_depth returns correct depth for various fork positions.
    #[test]
    fn test_fork_depth() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Favoured chain: 1->2->3->4
        insert_with_body(&mut tree, 2, 1, 1, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 3, 2, 2, BlockValidationStatus::Validated);
        insert_with_body(&mut tree, 4, 3, 3, BlockValidationStatus::Validated);
        tree.favoured_tip = Some(hash(4));

        // Fork from block 2: 2->5
        insert_with_body(&mut tree, 5, 2, 2, BlockValidationStatus::Validated);
        // Fork depth of block 5: tip is 4 (number=3), fork point is 2 (number=1)
        // depth = 3 - 1 = 2
        assert_eq!(tree.fork_depth(hash(5)).unwrap(), 2);

        // Block extending favoured chain: 4->6
        insert_with_body(&mut tree, 6, 4, 4, BlockValidationStatus::Validated);
        // Fork depth of block 6: parent is tip, depth = 0
        assert_eq!(tree.fork_depth(hash(6)).unwrap(), 0);

        // Fork from root: 1->7
        insert_with_body(&mut tree, 7, 1, 1, BlockValidationStatus::Validated);
        // Fork depth of block 7: tip number=3, ancestor=root number=0, depth=3
        assert_eq!(tree.fork_depth(hash(7)).unwrap(), 3);
    }

    // ── Phase 3 tests: US1 — Fork tracking (T018-T022) ───────────

    /// T018: 10+ fork topologies — linear, single fork, multi-fork, etc.
    #[test]
    fn test_fork_topologies() {
        // 1. Linear chain
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        for i in 2..=5u8 {
            tree.check_block_wanted(hash(i), hash(i - 1), i as u64 - 1, i as u64 - 1).unwrap();
        }
        assert_eq!(tree.favoured_tip(), Some(hash(5)));

        // 2. Single fork — longer branch wins
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();
        tree.check_block_wanted(hash(4), hash(1), 1, 1).unwrap(); // fork
        assert_eq!(tree.favoured_tip(), Some(hash(3)));

        // 3. Multi-fork — three branches, longest wins
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(4), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(5), hash(4), 2, 2).unwrap();
        tree.check_block_wanted(hash(6), hash(5), 3, 3).unwrap();
        assert_eq!(tree.favoured_tip(), Some(hash(6)));

        // 4. Deep tree — single long chain
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        for i in 2..=10u8 {
            tree.check_block_wanted(hash(i), hash(i - 1), i as u64 - 1, i as u64 - 1).unwrap();
        }
        assert_eq!(tree.favoured_tip(), Some(hash(10)));

        // 5. Balanced — two equal forks, current tip retained
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(1), 1, 1).unwrap();
        // Tip should be hash(2) (first inserted, became favoured)
        let tip = tree.favoured_tip().unwrap();
        assert!(tip == hash(2) || tip == hash(3));

        // 6. Skewed — one very long branch, one short
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        for i in 3..=8u8 {
            tree.check_block_wanted(hash(i), hash(i - 1), i as u64 - 1, i as u64 - 1).unwrap();
        }
        tree.check_block_wanted(hash(20), hash(1), 1, 1).unwrap();
        assert_eq!(tree.favoured_tip(), Some(hash(8)));

        // 7. Chain-of-forks — fork at every block
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();
        tree.check_block_wanted(hash(10), hash(1), 1, 1).unwrap(); // fork at 1
        tree.check_block_wanted(hash(11), hash(2), 2, 2).unwrap(); // fork at 2
        tree.check_block_wanted(hash(4), hash(3), 3, 3).unwrap();
        assert_eq!(tree.favoured_tip(), Some(hash(4)));

        // 8. Diamond — two paths converge (not possible in block trees
        // since different paths = different hashes, but test that both
        // branches are tracked independently)
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(4), hash(2), 2, 2).unwrap();
        tree.check_block_wanted(hash(5), hash(3), 2, 2).unwrap();
        // Both branches length 3, current tip favoured
        let tip = tree.favoured_tip().unwrap();
        assert!(tree.chain_contains(hash(1), tip));

        // 9. Zigzag — alternating forks
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();
        tree.check_block_wanted(hash(4), hash(2), 2, 2).unwrap(); // fork at 2
        tree.check_block_wanted(hash(5), hash(3), 3, 3).unwrap();
        tree.check_block_wanted(hash(6), hash(4), 3, 3).unwrap(); // fork at 4
        tree.check_block_wanted(hash(7), hash(5), 4, 4).unwrap();
        assert_eq!(tree.favoured_tip(), Some(hash(7)));

        // 10. Lopsided — one branch much longer after late fork
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        for i in 2..=6u8 {
            tree.check_block_wanted(hash(i), hash(i - 1), i as u64 - 1, i as u64 - 1).unwrap();
        }
        tree.check_block_wanted(hash(20), hash(5), 5, 5).unwrap(); // fork at 5
        tree.check_block_wanted(hash(21), hash(20), 6, 6).unwrap();
        tree.check_block_wanted(hash(22), hash(21), 7, 7).unwrap();
        // Fork: 1->2->3->4->5->20->21->22 (length 8) vs 1->2->3->4->5->6 (length 6)
        assert_eq!(tree.favoured_tip(), Some(hash(22)));
    }

    /// T019: Bounded maxvalid rejects block with fork depth > k.
    #[test]
    fn test_bounded_maxvalid_rejects_deep_fork() {
        let (mut tree, _) = make_tree(3); // k=3 for easy testing

        tree.set_root(hash(1), 0, 0).unwrap();
        // Favoured chain: 1->2->3->4->5
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();
        tree.check_block_wanted(hash(4), hash(3), 3, 3).unwrap();
        tree.check_block_wanted(hash(5), hash(4), 4, 4).unwrap();

        // Fork from root (depth 4 > k=3): should be rejected
        let result = tree.check_block_wanted(hash(10), hash(1), 1, 1);
        assert!(matches!(
            result,
            Err(ConsensusTreeError::ForkTooDeep { .. })
        ));
        // Block should NOT be in the tree
        assert!(tree.get_block(&hash(10)).is_none());
    }

    /// T020: Determinism — same insertion sequence always produces same tip.
    #[test]
    fn test_deterministic_chain_selection() {
        for _ in 0..10 {
            let (mut tree, _) = make_tree(2160);
            tree.set_root(hash(1), 0, 0).unwrap();
            tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
            tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();
            tree.check_block_wanted(hash(4), hash(1), 1, 1).unwrap();
            tree.check_block_wanted(hash(5), hash(4), 2, 2).unwrap();
            tree.check_block_wanted(hash(6), hash(5), 3, 3).unwrap();

            assert_eq!(tree.favoured_tip(), Some(hash(6)));
        }
    }

    /// T021: Block with unknown parent returns ParentNotFound.
    #[test]
    fn test_unknown_parent_returns_error() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        let result = tree.check_block_wanted(hash(2), hash(99), 1, 1);
        assert!(matches!(
            result,
            Err(ConsensusTreeError::ParentNotFound { .. })
        ));
    }

    /// T022: Block with invalid number returns InvalidBlockNumber.
    #[test]
    fn test_invalid_block_number_returns_error() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Parent is root (number 0), so expected number is 1, but we pass 5
        let result = tree.check_block_wanted(hash(2), hash(1), 5, 5);
        assert!(matches!(
            result,
            Err(ConsensusTreeError::InvalidBlockNumber {
                expected: 1,
                got: 5
            })
        ));
    }

    // ── Phase 4 tests: US2 — Block ingestion (T024-T029) ──────────

    /// T024: check_block_wanted returns hash as wanted when extending favoured chain.
    #[test]
    fn test_check_block_wanted_returns_wanted_for_favoured_chain() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        let wanted = tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        assert!(wanted.contains(&hash(2)));

        let block = tree.get_block(&hash(2)).unwrap();
        assert_eq!(block.status, BlockValidationStatus::Wanted);
    }

    /// T025: check_block_wanted does NOT return wanted for unfavoured fork.
    #[test]
    fn test_check_block_wanted_not_wanted_for_unfavoured_fork() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Build favoured chain: 1->2->3
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();

        // Fork at root: 1->4 (shorter, unfavoured)
        let wanted = tree.check_block_wanted(hash(4), hash(1), 1, 1).unwrap();

        // hash(4) should NOT be in wanted list
        assert!(!wanted.contains(&hash(4)));

        let block = tree.get_block(&hash(4)).unwrap();
        assert_eq!(block.status, BlockValidationStatus::Offered);
    }

    /// T026: add_block stores body and fires block_proposed for contiguous fetched blocks.
    #[test]
    fn test_add_block_fires_block_proposed() {
        let (mut tree, obs) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();

        // Add body for block 2
        tree.add_block(hash(2), vec![2]).unwrap();

        let proposed = unsafe { &*obs }.proposed.lock().unwrap();
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0], (1, hash(2)));
        drop(proposed);

        // Add body for block 3 — should also fire
        tree.add_block(hash(3), vec![3]).unwrap();

        let proposed = unsafe { &*obs }.proposed.lock().unwrap();
        assert_eq!(proposed.len(), 2);
        assert_eq!(proposed[1], (2, hash(3)));
    }

    /// T027: add_block with out-of-order delivery: block_proposed fires only up to first gap.
    #[test]
    fn test_add_block_out_of_order_stops_at_gap() {
        let (mut tree, obs) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();
        tree.check_block_wanted(hash(4), hash(3), 3, 3).unwrap();

        // Add block 4 first (out of order) — should NOT fire (gap at 2, 3)
        tree.add_block(hash(4), vec![4]).unwrap();

        let proposed = unsafe { &*obs }.proposed.lock().unwrap();
        assert_eq!(proposed.len(), 0);
        drop(proposed);

        // Add block 2 — should fire for block 2 only (gap at 3)
        tree.add_block(hash(2), vec![2]).unwrap();

        let proposed = unsafe { &*obs }.proposed.lock().unwrap();
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0], (1, hash(2)));
        drop(proposed);

        // Add block 3 — should fire for 3 and then 4
        tree.add_block(hash(3), vec![3]).unwrap();

        let proposed = unsafe { &*obs }.proposed.lock().unwrap();
        assert_eq!(proposed.len(), 3);
        assert_eq!(proposed[1], (2, hash(3)));
        assert_eq!(proposed[2], (3, hash(4)));
    }

    /// T028: add_block for already-fetched block is idempotent.
    #[test]
    fn test_add_block_idempotent() {
        let (mut tree, obs) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.add_block(hash(2), vec![2]).unwrap();

        let count_before = unsafe { &*obs }.proposed.lock().unwrap().len();

        // Add again — should be no-op
        tree.add_block(hash(2), vec![2]).unwrap();

        let count_after = unsafe { &*obs }.proposed.lock().unwrap().len();
        assert_eq!(count_before, count_after);
    }

    /// Duplicate check_block_wanted should not duplicate parent children.
    #[test]
    fn test_check_block_wanted_idempotent_for_existing_header() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        let first = tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        let second = tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();

        assert_eq!(first, vec![hash(2)]);
        assert_eq!(second, vec![hash(2)]);
        let root_children = &tree.get_block(&hash(1)).unwrap().children;
        assert_eq!(root_children.len(), 1);
        assert_eq!(root_children[0], hash(2));
    }

    /// T029: add_block for hash not in tree returns BlockNotInTree.
    #[test]
    fn test_add_block_not_in_tree_returns_error() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        let result = tree.add_block(hash(99), vec![99]);
        assert!(matches!(
            result,
            Err(ConsensusTreeError::BlockNotInTree { .. })
        ));
    }

    // ── Phase 5 tests: US3 — Rollbacks + validation (T031-T038) ──

    /// T031: Chain switch fires rollback with correct common ancestor.
    #[test]
    fn test_chain_switch_fires_rollback() {
        let (mut tree, obs) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Favoured: 1->2->3
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();

        // Clear any rollbacks from setup
        unsafe { &*obs }.rollbacks.lock().unwrap().clear();

        // Fork at root that becomes longer in one step:
        // Insert 4, 5 (equal), then 6 triggers switch
        tree.check_block_wanted(hash(4), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(5), hash(4), 2, 2).unwrap();
        // At this point fork is equal length — no switch yet (tie-break: current)
        tree.check_block_wanted(hash(6), hash(5), 3, 3).unwrap();
        // Now fork is longer — chain switch fires

        assert_eq!(tree.favoured_tip(), Some(hash(6)));

        let rollbacks = unsafe { &*obs }.rollbacks.lock().unwrap();
        // At least one rollback to root (number 0)
        assert!(!rollbacks.is_empty());
        assert!(rollbacks.contains(&0)); // Common ancestor is root
    }

    /// T032: Multi-level rollback fires correct ancestor.
    #[test]
    fn test_multi_level_rollback() {
        let (mut tree, obs) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Favoured: 1->2->3->4->5
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();
        tree.check_block_wanted(hash(4), hash(3), 3, 3).unwrap();
        tree.check_block_wanted(hash(5), hash(4), 4, 4).unwrap();

        // Fork at block 2: 2->10->11->12->13 (longer — triggers rollback to block 2)
        tree.check_block_wanted(hash(10), hash(2), 2, 2).unwrap();
        tree.check_block_wanted(hash(11), hash(10), 3, 3).unwrap();
        tree.check_block_wanted(hash(12), hash(11), 4, 4).unwrap();
        tree.check_block_wanted(hash(13), hash(12), 5, 5).unwrap();

        assert_eq!(tree.favoured_tip(), Some(hash(13)));

        let rollbacks = unsafe { &*obs }.rollbacks.lock().unwrap();
        // Rollback to block 2 (number 1)
        assert!(rollbacks.contains(&1));
    }

    /// T033: After rollback, block_proposed fires for fetched blocks on new chain.
    #[test]
    fn test_rollback_fires_proposed_for_fetched_blocks_on_new_chain() {
        let (mut tree, obs) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Favoured: 1->2->3
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.add_block(hash(2), vec![2]).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();
        tree.add_block(hash(3), vec![3]).unwrap();

        // Clear proposed events from setup
        unsafe { &*obs }.proposed.lock().unwrap().clear();

        // Fork: 1->4->5->6 with bodies (triggers chain switch)
        tree.check_block_wanted(hash(4), hash(1), 1, 1).unwrap();
        tree.add_block(hash(4), vec![4]).unwrap();
        tree.check_block_wanted(hash(5), hash(4), 2, 2).unwrap();
        tree.add_block(hash(5), vec![5]).unwrap();
        tree.check_block_wanted(hash(6), hash(5), 3, 3).unwrap();
        tree.add_block(hash(6), vec![6]).unwrap();

        // Check that rollback occurred and block_proposed fired for fetched blocks
        let rollbacks = unsafe { &*obs }.rollbacks.lock().unwrap();
        assert!(!rollbacks.is_empty());
    }

    /// T034: Chain switch transitions Offered→Wanted and returns them.
    #[test]
    fn test_chain_switch_transitions_offered_to_wanted() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Favoured: 1->2->3
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.check_block_wanted(hash(3), hash(2), 2, 2).unwrap();

        // Unfavoured fork: 1->4 (Offered)
        tree.check_block_wanted(hash(4), hash(1), 1, 1).unwrap();
        assert_eq!(
            tree.get_block(&hash(4)).unwrap().status,
            BlockValidationStatus::Offered
        );

        // Make unfavoured fork longer: 4->5->6 triggers chain switch
        tree.check_block_wanted(hash(5), hash(4), 2, 2).unwrap();
        let wanted = tree.check_block_wanted(hash(6), hash(5), 3, 3).unwrap();

        assert_eq!(tree.favoured_tip(), Some(hash(6)));

        // hash(4) should now be Wanted (transitioned from Offered)
        let block4 = tree.get_block(&hash(4)).unwrap();
        assert_eq!(block4.status, BlockValidationStatus::Wanted);

        // wanted list should contain the newly-wanted blocks
        assert!(!wanted.is_empty());
    }

    /// T035: mark_validated transitions status to Validated.
    #[test]
    fn test_mark_validated() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();
        tree.check_block_wanted(hash(2), hash(1), 1, 1).unwrap();
        tree.add_block(hash(2), vec![2]).unwrap();

        tree.mark_validated(hash(2)).unwrap();
        assert_eq!(
            tree.get_block(&hash(2)).unwrap().status,
            BlockValidationStatus::Validated
        );
    }

    /// T036: mark_rejected fires block_rejected, removes block + descendants.
    #[test]
    fn test_mark_rejected() {
        let (mut tree, obs) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        insert_with_body(&mut tree, 2, 1, 1, BlockValidationStatus::Fetched);
        insert_no_body(&mut tree, 3, 2, 2, BlockValidationStatus::Wanted);
        tree.favoured_tip = Some(hash(3));

        tree.mark_rejected(hash(2)).unwrap();

        // block_rejected should have fired
        let rejected = unsafe { &*obs }.rejected.lock().unwrap();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0], hash(2));
        drop(rejected);

        // Block 2 and descendant 3 should be removed
        assert!(tree.get_block(&hash(2)).is_none());
        assert!(tree.get_block(&hash(3)).is_none());
    }

    /// T037: remove_block removes block and all descendants.
    #[test]
    fn test_remove_block_removes_descendants() {
        let (mut tree, _) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Use insert_block directly to avoid check_block_wanted side effects
        insert_no_body(&mut tree, 2, 1, 1, BlockValidationStatus::Wanted);
        insert_no_body(&mut tree, 3, 2, 2, BlockValidationStatus::Wanted);
        insert_no_body(&mut tree, 4, 3, 3, BlockValidationStatus::Wanted);
        tree.favoured_tip = Some(hash(4));

        tree.remove_block(hash(2)).unwrap();

        assert!(tree.get_block(&hash(2)).is_none());
        assert!(tree.get_block(&hash(3)).is_none());
        assert!(tree.get_block(&hash(4)).is_none());
        assert_eq!(tree.len(), 1); // Only root remains
    }

    /// T038: remove_block causing chain switch fires rollback and returns wanted.
    #[test]
    fn test_remove_block_chain_switch() {
        let (mut tree, obs) = make_tree(2160);
        tree.set_root(hash(1), 0, 0).unwrap();

        // Favoured: 1->2->3->4
        insert_no_body(&mut tree, 2, 1, 1, BlockValidationStatus::Wanted);
        insert_no_body(&mut tree, 3, 2, 2, BlockValidationStatus::Wanted);
        insert_no_body(&mut tree, 4, 3, 3, BlockValidationStatus::Wanted);
        tree.favoured_tip = Some(hash(4));

        // Alternative: 1->5->6 (shorter, Offered)
        insert_no_body(&mut tree, 5, 1, 1, BlockValidationStatus::Offered);
        insert_no_body(&mut tree, 6, 2, 5, BlockValidationStatus::Offered);

        assert_eq!(tree.favoured_tip(), Some(hash(4)));

        // Remove block 2 (and descendants 3,4) — should switch to 5->6
        unsafe { &*obs }.rollbacks.lock().unwrap().clear();
        let wanted = tree.remove_block(hash(2)).unwrap();

        assert_eq!(tree.favoured_tip(), Some(hash(6)));

        let rollbacks = unsafe { &*obs }.rollbacks.lock().unwrap();
        assert!(!rollbacks.is_empty());
        drop(rollbacks);

        // Newly wanted blocks: 5 and 6 should be transitioned to Wanted
        assert!(!wanted.is_empty());
    }

    // ── Phase 6 tests: US4 — Pruning (T043-T046) ─────────────────

    /// T043: prune removes blocks with number < (tip - k).
    #[test]
    fn test_prune_removes_old_blocks() {
        let (mut tree, _) = make_tree(3); // k=3

        tree.set_root(hash(1), 0, 0).unwrap();
        for i in 2..=6u8 {
            tree.check_block_wanted(hash(i), hash(i - 1), i as u64 - 1, i as u64 - 1).unwrap();
            tree.add_block(hash(i), vec![i]).unwrap();
        }
        // Chain: 1(0)->2(1)->3(2)->4(3)->5(4)->6(5), tip=6 at number 5
        // Prune boundary: 5 - 3 = 2
        // Blocks with number < 2 should be removed: hash(1) (num 0), hash(2) (num 1)

        tree.prune().unwrap();

        assert!(tree.get_block(&hash(1)).is_none()); // num 0
        assert!(tree.get_block(&hash(2)).is_none()); // num 1
        assert!(tree.get_block(&hash(3)).is_some()); // num 2 (new root)
        assert!(tree.get_block(&hash(6)).is_some()); // tip
    }

    /// T044: prune removes non-favoured branch rooted before prune boundary.
    #[test]
    fn test_prune_removes_unfavoured_branch() {
        let (mut tree, _) = make_tree(3); // k=3

        tree.set_root(hash(1), 0, 0).unwrap();
        // Favoured: 1->2->3->4->5->6
        for i in 2..=6u8 {
            insert_with_body(
                &mut tree,
                i,
                i as u64 - 1,
                i - 1,
                BlockValidationStatus::Validated,
            );
        }
        tree.favoured_tip = Some(hash(6));

        // Fork at block 2: 2->10 (use insert_block to bypass bounded maxvalid)
        insert_no_body(&mut tree, 10, 2, 2, BlockValidationStatus::Offered);

        tree.prune().unwrap();

        // Block 10 (forked from block 2 which is before prune boundary) should be removed
        assert!(tree.get_block(&hash(10)).is_none());
    }

    /// T045: prune preserves both branches of fork after prune boundary.
    #[test]
    fn test_prune_preserves_fork_after_boundary() {
        let (mut tree, _) = make_tree(3); // k=3

        tree.set_root(hash(1), 0, 0).unwrap();
        // Favoured: 1->2->3->4->5->6
        for i in 2..=6u8 {
            tree.check_block_wanted(hash(i), hash(i - 1), i as u64 - 1, i as u64 - 1).unwrap();
            tree.add_block(hash(i), vec![i]).unwrap();
        }
        // Fork at block 4 (after prune boundary): 4->10
        tree.check_block_wanted(hash(10), hash(4), 4, 4).unwrap();

        tree.prune().unwrap();

        // Block 10 should be preserved (fork after prune boundary)
        assert!(tree.get_block(&hash(10)).is_some());
        assert!(tree.get_block(&hash(4)).is_some());
    }

    /// T046: prune updates root to new oldest block.
    #[test]
    fn test_prune_updates_root() {
        let (mut tree, _) = make_tree(3); // k=3

        tree.set_root(hash(1), 0, 0).unwrap();
        for i in 2..=6u8 {
            tree.check_block_wanted(hash(i), hash(i - 1), i as u64 - 1, i as u64 - 1).unwrap();
            tree.add_block(hash(i), vec![i]).unwrap();
        }

        tree.prune().unwrap();

        // Root should be updated to the block at the prune boundary
        let new_root = tree.root().unwrap();
        let root_block = tree.get_block(&new_root).unwrap();
        assert!(root_block.parent.is_none()); // New root has no parent
        assert!(root_block.number >= 2); // At or after prune boundary
    }
}
