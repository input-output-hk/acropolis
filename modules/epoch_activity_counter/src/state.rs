//! Acropolis epoch activity counter: state storage

use acropolis_common::{
    crypto::keyhash,
    messages::{CardanoMessage, EpochActivityMessage, Message},
    BlockInfo, KeyHash,
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

pub struct State {
    // Map of counts by VRF key hashes
    vrf_vkey_hashes: HashMap<KeyHash, usize>,

    // Total blocks seen this epoch
    total_blocks: usize,

    // Total fees seen this epoch
    total_fees: u64,
}

impl State {
    // Constructor
    pub fn new() -> Self {
        Self {
            vrf_vkey_hashes: HashMap::new(),
            total_blocks: 0,
            total_fees: 0,
        }
    }

    // Handle a block minting, taking the SPO's VRF vkey
    pub fn handle_mint(&mut self, _block: &BlockInfo, vrf_vkey: &[u8]) {
        self.total_blocks += 1;

        // Count one on this hash
        *(self.vrf_vkey_hashes.entry(keyhash(vrf_vkey)).or_insert(0)) += 1;
    }

    // Handle block fees
    pub fn handle_fees(&mut self, _block: &BlockInfo, total_fees: u64) {
        self.total_fees += total_fees;
    }

    // Handle end of epoch, returns message to be published
    pub fn end_epoch(&mut self, block: &BlockInfo, epoch: u64) -> Arc<Message> {
        info!(
            epoch,
            total_blocks = self.total_blocks,
            unique_vrf_keys = self.vrf_vkey_hashes.len(),
            total_fees = self.total_fees,
            "End of epoch"
        );

        let message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::EpochActivity(EpochActivityMessage {
                epoch: epoch,
                total_blocks: self.total_blocks,
                total_fees: self.total_fees,
                vrf_vkey_hashes: self.vrf_vkey_hashes.drain().collect(),
            }),
        )));

        self.total_blocks = 0;
        self.total_fees = 0;

        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        crypto::keyhash,
        messages::{CardanoMessage, Message},
        BlockInfo, BlockStatus, Era,
    };

    fn make_block(epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 99,
            number: 42,
            hash: Vec::new(),
            epoch,
            new_epoch: false,
            era: Era::Conway,
        }
    }

    #[test]
    fn initial_state_is_zeroed() {
        let state = State::new();
        assert_eq!(state.total_blocks, 0);
        assert_eq!(state.total_fees, 0);
        assert!(state.vrf_vkey_hashes.is_empty());
    }

    #[test]
    fn handle_mint_single_vrf_records_counts() {
        let mut state = State::new();
        let vrf = b"vrf_key";
        let block = make_block(100);
        state.handle_mint(&block, vrf);
        state.handle_mint(&block, vrf);

        assert_eq!(state.total_blocks, 2);
        assert_eq!(state.vrf_vkey_hashes.len(), 1);
        assert_eq!(state.vrf_vkey_hashes.get(&keyhash(vrf)), Some(&2));
    }

    #[test]
    fn handle_mint_multiple_vrf_records_counts() {
        let mut state = State::new();
        let block = make_block(100);
        state.handle_mint(&block, b"vrf_1");
        state.handle_mint(&block, b"vrf_2");
        state.handle_mint(&block, b"vrf_2");

        assert_eq!(state.total_blocks, 3);
        assert_eq!(state.vrf_vkey_hashes.len(), 2);
        assert_eq!(
            state
                .vrf_vkey_hashes
                .iter()
                .find(|(k, _)| *k == &keyhash(b"vrf_1"))
                .map(|(_, v)| *v),
            Some(1)
        );
        assert_eq!(
            state
                .vrf_vkey_hashes
                .iter()
                .find(|(k, _)| *k == &keyhash(b"vrf_2"))
                .map(|(_, v)| *v),
            Some(2)
        );
    }

    #[test]
    fn handle_fees_counts_fees() {
        let mut state = State::new();
        let block = make_block(100);
        state.handle_fees(&block, 100);
        state.handle_fees(&block, 250);

        assert_eq!(state.total_fees, 350);
    }

    #[test]
    fn end_epoch_resets_and_returns_message() {
        let mut state = State::new();
        let block = make_block(101);
        state.handle_mint(&block, b"vrf_1");
        state.handle_fees(&block, 123);

        // Check the message returned
        let msg = state.end_epoch(&block, 100);
        match msg.as_ref() {
            Message::Cardano((block, CardanoMessage::EpochActivity(ea))) => {
                assert_eq!(block.epoch, 101);
                assert_eq!(ea.epoch, 100);
                assert_eq!(ea.total_blocks, 1);
                assert_eq!(ea.total_fees, 123);
                assert_eq!(ea.vrf_vkey_hashes.len(), 1);
                assert_eq!(
                    ea.vrf_vkey_hashes
                        .iter()
                        .find(|(k, _)| k == &keyhash(b"vrf_1"))
                        .map(|(_, v)| *v),
                    Some(1)
                );
            }
            _ => panic!("Expected EpochActivity message"),
        }

        // State must be reset
        assert_eq!(state.total_blocks, 0);
        assert_eq!(state.total_fees, 0);
        assert!(state.vrf_vkey_hashes.is_empty());
    }
}
