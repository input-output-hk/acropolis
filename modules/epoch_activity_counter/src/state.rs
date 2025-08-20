//! Acropolis epoch activity counter: state storage

use acropolis_common::{
    crypto::keyhash,
    messages::{CardanoMessage, EpochActivityMessage, Message},
    BlockInfo, KeyHash,
};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use tracing::{error, info};

pub struct State {
    // Current epoch number
    current_epoch: u64,

    // Map of counts by VRF key hashes
    blocks_minted: HashMap<KeyHash, usize>,

    // Total blocks seen this epoch
    total_blocks: usize,

    // Maps of fees by vrf key hashes
    fees: HashMap<KeyHash, u64>,

    // Total fees seen this epoch
    total_fees: u64,

    // Blocks seen this epoch
    // removed when we calculate the fee of the block
    // and cleared when we end the epoch
    blocks: VecDeque<(u64, KeyHash)>,

    // History of epochs (disabled by default)
    epoch_history: Option<BTreeMap<u64, EpochActivityMessage>>,
}

impl State {
    // Constructor
    pub fn new(store_history: bool) -> Self {
        Self {
            current_epoch: 0,
            blocks_minted: HashMap::new(),
            total_blocks: 0,
            total_fees: 0,
            fees: HashMap::new(),
            blocks: VecDeque::new(),
            epoch_history: if store_history {
                Some(BTreeMap::new())
            } else {
                None
            },
        }
    }

    // Handle a block minting, taking the SPO's VRF vkey
    pub fn handle_mint(&mut self, block: &BlockInfo, vrf_vkey: Option<&[u8]>) {
        self.total_blocks += 1;

        if let Some(vrf_vkey) = vrf_vkey {
            // Count one on this hash
            *(self.blocks_minted.entry(keyhash(vrf_vkey)).or_insert(0)) += 1;

            // Add the block to the queue
            self.blocks.push_back((block.number, keyhash(vrf_vkey)));
        }
    }

    // Handle block fees
    pub fn handle_fees(&mut self, block: &BlockInfo, block_fee: u64) {
        self.total_fees += block_fee;

        // find the block in the queue
        loop {
            let Some((front_number, vrf_key_hash)) = self.blocks.pop_front() else {
                break;
            };
            if block.number > front_number {
                // if CardanoMessage::BlockFees is received before CardanoMessage::BlockHeader.
                error!("CardanoMessage::BlockFees is received before CardanoMessage::BlockHeader.");
            } else if front_number == block.number {
                // add this fee to fees for this vrf key hash
                *(self.fees.entry(vrf_key_hash).or_insert(0)) += block_fee;
                break;
            }
        }
    }

    // Handle end of epoch, returns message to be published
    pub fn end_epoch(&mut self, block: &BlockInfo, epoch: u64) -> Arc<Message> {
        info!(
            epoch,
            total_blocks = self.total_blocks,
            unique_vrf_keys = self.blocks_minted.len(),
            total_fees = self.total_fees,
            "End of epoch"
        );

        let epoch_activity = self.get_current_epoch();

        if let Some(history) = &mut self.epoch_history {
            history.insert(epoch, epoch_activity.clone());
        }

        let message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::EpochActivity(epoch_activity),
        )));

        self.current_epoch = epoch + 1;
        self.total_blocks = 0;
        self.blocks_minted.clear();
        self.total_fees = 0;
        self.fees.clear();
        self.blocks.clear();

        message
    }

    pub fn get_current_epoch(&self) -> EpochActivityMessage {
        EpochActivityMessage {
            epoch: self.current_epoch,
            total_blocks: self.total_blocks,
            total_fees: self.total_fees,
            vrf_vkey_hashes: self.blocks_minted.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            fees: self.fees.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        }
    }

    pub fn get_historical_epoch(
        &self,
        epoch: u64,
    ) -> Result<Option<&EpochActivityMessage>, anyhow::Error> {
        let history = self
            .epoch_history
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Historical epoch storage is disabled"))?;
        Ok(history.get(&epoch))
    }

    /// Get current epoch's blocks minted for each vrf key hash
    /// ### NOTE:
    /// This function only works when `store_history` is enabled.
    pub fn get_blocks_minted_by_pools(&self, vrf_key_hashes: &Vec<KeyHash>) -> Vec<u64> {
        vrf_key_hashes
            .iter()
            .map(|key_hash| self.blocks_minted.get(key_hash).map(|v| *v as u64).unwrap_or(0))
            .collect()
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
        let state = State::new(false);
        assert_eq!(state.total_blocks, 0);
        assert_eq!(state.total_fees, 0);
        assert!(state.blocks_minted.is_empty());
    }

    #[test]
    fn handle_mint_single_vrf_records_counts() {
        let mut state = State::new(false);
        let vrf = b"vrf_key";
        let mut block = make_block(100);
        state.handle_mint(&block, Some(vrf));
        state.handle_fees(&block, 100);

        block.number += 1;
        state.handle_mint(&block, Some(vrf));
        state.handle_fees(&block, 200);

        assert_eq!(state.total_blocks, 2);
        assert_eq!(state.blocks_minted.len(), 1);
        assert_eq!(state.blocks_minted.get(&keyhash(vrf)), Some(&2));
        assert_eq!(state.fees.get(&keyhash(vrf)), Some(&300));
        assert!(state.blocks.is_empty());
    }

    #[test]
    fn handle_mint_multiple_vrf_records_counts() {
        let mut state = State::new(false);
        let mut block = make_block(100);
        state.handle_mint(&block, Some(b"vrf_1"));
        block.number += 1;
        state.handle_mint(&block, Some(b"vrf_2"));
        block.number += 1;
        state.handle_mint(&block, Some(b"vrf_2"));

        assert_eq!(state.total_blocks, 3);
        assert_eq!(state.blocks_minted.len(), 2);
        assert_eq!(
            state.blocks_minted.iter().find(|(k, _)| *k == &keyhash(b"vrf_1")).map(|(_, v)| *v),
            Some(1)
        );
        assert_eq!(
            state.blocks_minted.iter().find(|(k, _)| *k == &keyhash(b"vrf_2")).map(|(_, v)| *v),
            Some(2)
        );

        block = make_block(100);
        state.handle_fees(&block, 100);
        block.number += 1;
        state.handle_fees(&block, 200);
        block.number += 1;
        state.handle_fees(&block, 300);

        assert_eq!(state.blocks.len(), 0);
        assert_eq!(state.fees.len(), 2);
        assert_eq!(
            state.fees.iter().find(|(k, _)| *k == &keyhash(b"vrf_1")).map(|(_, v)| *v),
            Some(100)
        );
        assert_eq!(
            state.fees.iter().find(|(k, _)| *k == &keyhash(b"vrf_2")).map(|(_, v)| *v),
            Some(500)
        );
    }

    #[test]
    fn handle_fees_counts_fees() {
        let mut state = State::new(false);
        let mut block = make_block(100);
        state.blocks = VecDeque::from([
            (block.number, keyhash(b"vrf_1")),
            (block.number + 1, keyhash(b"vrf_2")),
        ]);

        state.handle_fees(&block, 100);
        block.number += 1;
        state.handle_fees(&block, 250);

        assert_eq!(state.total_fees, 350);
    }

    #[test]
    fn end_epoch_resets_and_returns_message() {
        let mut state = State::new(false);
        let block = make_block(0);
        state.handle_mint(&block, Some(b"vrf_1"));
        state.handle_fees(&block, 123);

        // Check the message returned
        let msg = state.end_epoch(&block, 0);
        match msg.as_ref() {
            Message::Cardano((block, CardanoMessage::EpochActivity(ea))) => {
                assert_eq!(block.epoch, 0);
                assert_eq!(ea.epoch, 0);
                assert_eq!(ea.total_blocks, 1);
                assert_eq!(ea.total_fees, 123);
                assert_eq!(ea.vrf_vkey_hashes.len(), 1);
                assert_eq!(ea.fees.len(), 1);
                assert_eq!(
                    ea.vrf_vkey_hashes
                        .iter()
                        .find(|(k, _)| k == &keyhash(b"vrf_1"))
                        .map(|(_, v)| *v),
                    Some(1)
                );
                assert_eq!(
                    ea.fees.iter().find(|(k, _)| k == &keyhash(b"vrf_1")).map(|(_, v)| *v),
                    Some(123)
                );
            }
            _ => panic!("Expected EpochActivity message"),
        }

        // State must be reset
        assert_eq!(state.current_epoch, 1);
        assert_eq!(state.total_blocks, 0);
        assert_eq!(state.total_fees, 0);
        assert!(state.blocks_minted.is_empty());
    }

    #[test]
    fn end_epoch_saves_history() {
        let mut state = State::new(true);
        let block = make_block(200);
        state.handle_mint(&block, Some(b"vrf_1"));
        state.handle_fees(&block, 50);

        state.end_epoch(&block, 199);
        assert!(state.blocks_minted.is_empty());
        assert!(state.fees.is_empty());
        assert!(state.blocks.is_empty());

        // Use the public API method
        let history = state
            .get_historical_epoch(199)
            .expect("history disabled in test")
            .expect("epoch history missing");

        assert_eq!(history.total_blocks, 1);
        assert_eq!(history.total_fees, 50);
        assert_eq!(history.fees.len(), 1);
        assert_eq!(
            history.fees.iter().find(|(k, _)| k == &keyhash(b"vrf_1")).map(|(_, v)| *v),
            Some(50)
        );
    }
}
