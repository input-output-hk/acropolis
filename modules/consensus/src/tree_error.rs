//! Error types for consensus tree operations.

use acropolis_common::BlockHash;

/// Errors returned by [`ConsensusTree`](crate::consensus_tree::ConsensusTree) operations.
#[derive(Debug, thiserror::Error)]
pub enum ConsensusTreeError {
    /// The offered block's parent hash is not present in the tree.
    #[error("parent not found: {hash}")]
    ParentNotFound { hash: BlockHash },

    /// The offered block's number does not equal parent number + 1.
    #[error("invalid block number: expected {expected}, got {got}")]
    InvalidBlockNumber { expected: u64, got: u64 },

    /// A block hash referenced by an operation is not in the tree.
    #[error("block not in tree: {hash}")]
    BlockNotInTree { hash: BlockHash },

    /// The candidate chain forks from the current chain deeper than k blocks.
    #[error("fork too deep: depth {fork_depth} exceeds max {max_k}")]
    ForkTooDeep { fork_depth: u64, max_k: u64 },
}
