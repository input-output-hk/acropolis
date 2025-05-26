//! Acropolis epoch activity counter: state storage

use acropolis_common::{BlockInfo, messages::{Message, CardanoMessage, EpochActivityMessage}};
use std::sync::Arc;
use tracing::info;
use std::collections::HashMap;

pub struct State {

    // Map of counts by VRF keys
    vrf_vkeys: HashMap<Vec<u8>, usize>,

    // Total blocks seen this epoch
    total_blocks: usize,

    // Total fees seen this epoch
    total_fees: u64,
}

impl State {
    // Constructor
    pub fn new() -> Self {
        Self {
            vrf_vkeys: HashMap::new(),
            total_blocks: 0,
            total_fees: 0,
        }
    }

    // Handle a block minting, taking the SPO's VRF vkey
    pub fn handle_mint(&mut self, _block: &BlockInfo, vrf_vkey: &[u8]) {
        self.total_blocks += 1;
        *(self.vrf_vkeys.entry(vrf_vkey.to_vec()).or_insert(0)) += 1;
    }

    // Handle block fees
    pub fn handle_fees(&mut self, _block: &BlockInfo, total_fees: u64) {
        self.total_fees += total_fees;
    }

    // Handle end of epoch, returns message to be published
    pub fn end_epoch(&mut self, block: &BlockInfo) -> Arc<Message> {
        info!("End of epoch {} - {} total blocks, {} unique VRF keys, total fees {}",
              block.epoch-1, self.total_blocks, self.vrf_vkeys.len(),
              self.total_fees);

        let message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::EpochActivity(EpochActivityMessage {
                epoch: block.epoch-1,
                total_blocks: self.total_blocks,
                total_fees: self.total_fees,
                vrf_vkeys: self.vrf_vkeys.drain().collect(),
            }))
        ));

        self.total_blocks = 0;
        self.total_fees = 0;

        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{BlockStatus, Era, BlockInfo, messages::{Message, CardanoMessage}};

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
        assert!(state.vrf_vkeys.is_empty());
    }

    #[test]
    fn handle_mint_single_vrf_records_counts() {
        let mut state = State::new();
        let vrf = b"vrf_key";
        let block = make_block(100);
        state.handle_mint(&block, vrf);
        state.handle_mint(&block, vrf);

        assert_eq!(state.total_blocks, 2);
        assert_eq!(state.vrf_vkeys.len(), 1);
        assert_eq!(state.vrf_vkeys.get(&vrf.to_vec()), Some(&2));
    }

    #[test]
    fn handle_mint_multiple_vrf_records_counts() {
        let mut state = State::new();
        let block = make_block(100);
        state.handle_mint(&block, b"vrf_1");
        state.handle_mint(&block, b"vrf_2");
        state.handle_mint(&block, b"vrf_2");

        assert_eq!(state.total_blocks, 3);
        assert_eq!(state.vrf_vkeys.len(), 2);
        assert_eq!(state.vrf_vkeys.iter()
                     .find(|(k, _)| *k == &b"vrf_1".to_vec())
                     .map(|(_, v)| *v),
                   Some(1));
        assert_eq!(state.vrf_vkeys.iter()
                     .find(|(k, _)| *k == &b"vrf_2".to_vec())
                     .map(|(_, v)| *v),
                   Some(2));
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
        let msg = state.end_epoch(&block);
        match msg.as_ref() {
            Message::Cardano((block, CardanoMessage::EpochActivity(ea))) => {
                assert_eq!(block.epoch, 101);
                assert_eq!(ea.epoch, 100);
                assert_eq!(ea.total_blocks, 1);
                assert_eq!(ea.total_fees, 123);
                assert_eq!(ea.vrf_vkeys.len(), 1);
                assert_eq!(ea.vrf_vkeys.iter()
                     .find(|(k, _)| k == &b"vrf_1".to_vec())
                     .map(|(_, v)| *v),
                   Some(1));
            }
            _ => panic!("Expected EpochActivity message"),
        }

        // State must be reset
        assert_eq!(state.total_blocks, 0);
        assert_eq!(state.total_fees, 0);
        assert!(state.vrf_vkeys.is_empty());
    }
}
