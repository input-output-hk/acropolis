//! Observer trait for consensus tree events.

use acropolis_common::BlockHash;

/// Callback receiver for consensus tree events.
///
/// The owning module implements this trait to translate tree events
/// into message bus publications (e.g. `cardano.block.proposed`,
/// `cardano.block.rejected`).
pub trait ConsensusTreeObserver {
    /// A block is ready to be proposed for validation/application.
    ///
    /// Called in strictly ascending block-number order with no gaps.
    fn block_proposed(&self, number: u64, hash: BlockHash, body: &[u8]);

    /// The favoured chain has switched — rollback to the given block number.
    ///
    /// All state applied after `to_block_number` should be reverted.
    fn rollback(&self, to_block_number: u64);

    /// A block has been rejected by validation.
    ///
    /// PNI should sanction the peers that provided this block.
    fn block_rejected(&self, hash: BlockHash);
}
