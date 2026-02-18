//! Block representation within the consensus tree.

use acropolis_common::BlockHash;

/// Tracks where a block is in the fetch-validate lifecycle.
///
/// Blocks on unfavoured forks start as `Offered` and are promoted to
/// `Wanted` when a chain switch makes their fork favoured. Only
/// `Wanted` blocks are returned to the caller for fetching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockValidationStatus {
    /// Header known; on unfavoured fork; not yet fetched.
    Offered,
    /// Fetch requested; on favoured chain.
    Wanted,
    /// Body received; awaiting validation.
    Fetched,
    /// Passed validation; safe to apply.
    Validated,
    /// Failed validation; will be removed immediately.
    Rejected,
}

/// A node in the consensus tree representing a block header (and
/// optionally its body) within the volatile window.
#[derive(Debug, Clone)]
pub struct TreeBlock {
    /// 32-byte block hash (identity key).
    pub hash: BlockHash,
    /// Block height.
    pub number: u64,
    /// Slot number.
    pub slot: u64,
    /// Raw block body; `None` until fetched.
    pub body: Option<Vec<u8>>,
    /// Parent block hash; `None` for the root.
    pub parent: Option<BlockHash>,
    /// Child block hashes.
    pub children: Vec<BlockHash>,
    /// Current lifecycle status.
    pub status: BlockValidationStatus,
}

impl TreeBlock {
    /// Create a new tree block with no body and no children.
    pub fn new(
        hash: BlockHash,
        number: u64,
        slot: u64,
        parent: Option<BlockHash>,
        status: BlockValidationStatus,
    ) -> Self {
        Self {
            hash,
            number,
            slot,
            body: None,
            parent,
            children: Vec::new(),
            status,
        }
    }
}
