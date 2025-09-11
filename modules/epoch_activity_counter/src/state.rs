//! Acropolis epoch activity counter: state storage

use acropolis_common::{
    crypto::keyhash, messages::EpochActivityMessage, BlockHash, BlockInfo, KeyHash,
};
use imbl::{HashMap, Vector};
use tracing::{error, info};

use crate::store_config::StoreConfig;

#[derive(Default, Debug, Clone)]
pub struct State {
    store_config: StoreConfig,

    // block number
    block: u64,

    // epoch number N
    epoch: u64,

    // Map of counts by VRF key hashes
    blocks_minted: HashMap<KeyHash, usize>,

    // blocks seen this epoch
    epoch_blocks: usize,

    // fees seen this epoch
    epoch_fees: u64,

    // Total blocks minted till block number
    // Keyed by vrf_key_hash
    total_blocks_minted: HashMap<KeyHash, u64>,

    // block hashes by vrf_key_hash
    block_hashes: Option<HashMap<KeyHash, Vector<BlockHash>>>,
}

impl State {
    // Constructor
    pub fn new(store_config: &StoreConfig) -> Self {
        Self {
            store_config: store_config.clone(),
            block: 0,
            epoch: 0,
            blocks_minted: HashMap::new(),
            epoch_blocks: 0,
            epoch_fees: 0,
            total_blocks_minted: HashMap::new(),
            block_hashes: if store_config.store_block_hashes {
                Some(HashMap::new())
            } else {
                None
            },
        }
    }

    pub fn is_block_hashes_enabled(&self) -> bool {
        self.store_config.store_block_hashes
    }

    // Handle a block minting, taking the SPO's VRF vkey
    pub fn handle_mint(&mut self, block: &BlockInfo, vrf_vkey: Option<&[u8]>) {
        self.epoch_blocks += 1;
        if let Some(vrf_vkey) = vrf_vkey {
            let vrf_key_hash = keyhash(vrf_vkey);
            // Count one on this hash
            *(self.blocks_minted.entry(vrf_key_hash.clone()).or_insert(0)) += 1;
            *(self.total_blocks_minted.entry(vrf_key_hash.clone()).or_insert(0)) += 1;

            if let Some(block_hashes) = self.block_hashes.as_mut() {
                let block_hash: Option<BlockHash> = block
                    .hash
                    .clone()
                    .try_into()
                    .inspect_err(|_| error!("Block hash is not 32 bytes"))
                    .ok();
                if let Some(block_hash) = block_hash {
                    block_hashes
                        .entry(vrf_key_hash.clone())
                        .or_insert_with(Vector::new)
                        .push_back(block_hash);
                }
            }
        }
    }

    // Handle block fees
    pub fn handle_fees(&mut self, _block: &BlockInfo, block_fee: u64) {
        self.epoch_fees += block_fee;
    }

    // Handle end of epoch, returns message to be published
    // block is the first block of coming epoch
    pub fn end_epoch(&mut self, block_info: &BlockInfo) -> EpochActivityMessage {
        info!(
            epoch = block_info.epoch - 1,
            blocks = self.epoch_blocks,
            unique_vrf_keys = self.blocks_minted.len(),
            fees = self.epoch_fees,
            "End of epoch"
        );

        let epoch_activity = self.get_epoch_info();

        // clear epoch state
        self.block = block_info.number;
        self.epoch = block_info.epoch;
        self.blocks_minted.clear();
        self.epoch_blocks = 0;
        self.epoch_fees = 0;

        epoch_activity
    }

    pub fn get_epoch_info(&self) -> EpochActivityMessage {
        EpochActivityMessage {
            epoch: self.epoch,
            total_blocks: self.epoch_blocks,
            total_fees: self.epoch_fees,
            vrf_vkey_hashes: self.blocks_minted.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        }
    }

    /// Get current epoch's blocks minted for each vrf key hash
    pub fn get_blocks_minted_by_pools(&self, vrf_key_hashes: &Vec<KeyHash>) -> Vec<u64> {
        vrf_key_hashes
            .iter()
            .map(|key_hash| self.blocks_minted.get(key_hash).map(|v| *v as u64).unwrap_or(0))
            .collect()
    }

    /// Get epoch's total blocks minted for each vrf key hash till current block number
    pub fn get_total_blocks_minted_by_pools(&self, vrf_key_hashes: &Vec<KeyHash>) -> Vec<u64> {
        vrf_key_hashes
            .iter()
            .map(|key_hash| self.total_blocks_minted.get(key_hash).map(|v| *v as u64).unwrap_or(0))
            .collect()
    }

    /// Get Block Hashes by Vrf Key Hash
    pub fn get_block_hashes(&self, vrf_key_hash: &KeyHash) -> Vec<BlockHash> {
        let Some(block_hashes) = self.block_hashes.as_ref() else {
            return vec![];
        };

        block_hashes
            .get(vrf_key_hash)
            .map(|hashes| hashes.into_iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use acropolis_common::{
        crypto::keyhash,
        state_history::{StateHistory, StateHistoryStore},
        BlockHash, BlockInfo, BlockStatus, Era,
    };
    use tokio::sync::Mutex;

    fn make_block(epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 0,
            number: epoch * 10,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: 99,
            new_epoch: false,
            timestamp: 99999,
            era: Era::Conway,
        }
    }

    fn make_rolled_back_block(epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 0,
            number: epoch * 10,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: 99,
            new_epoch: false,
            timestamp: 99999,
            era: Era::Conway,
        }
    }

    #[test]
    fn initial_state_is_zeroed() {
        let state = State::new(&StoreConfig::default());
        assert_eq!(state.epoch_blocks, 0);
        assert_eq!(state.epoch_fees, 0);
        assert!(state.blocks_minted.is_empty());
        assert!(state.block_hashes.is_none());
    }

    #[test]
    fn handle_mint_single_vrf_records_counts() {
        let mut state = State::new(&StoreConfig::default());
        let vrf = b"vrf_key";
        let mut block = make_block(100);
        state.handle_mint(&block, Some(vrf));
        state.handle_fees(&block, 100);

        block.number += 1;
        state.handle_mint(&block, Some(vrf));
        state.handle_fees(&block, 200);

        assert_eq!(state.epoch_blocks, 2);
        assert_eq!(state.blocks_minted.len(), 1);
        assert_eq!(state.blocks_minted.get(&keyhash(vrf)), Some(&2));
        assert_eq!(state.total_blocks_minted.get(&keyhash(vrf)), Some(&2));
    }

    #[test]
    fn handle_mint_multiple_vrf_records_counts() {
        let mut state = State::new(&StoreConfig::default());
        let mut block = make_block(100);
        state.handle_mint(&block, Some(b"vrf_1"));
        block.number += 1;
        state.handle_mint(&block, Some(b"vrf_2"));
        block.number += 1;
        state.handle_mint(&block, Some(b"vrf_2"));

        assert_eq!(state.epoch_blocks, 3);
        assert_eq!(state.blocks_minted.len(), 2);
        assert_eq!(
            state.blocks_minted.iter().find(|(k, _)| *k == &keyhash(b"vrf_1")).map(|(_, v)| *v),
            Some(1)
        );
        assert_eq!(
            state.blocks_minted.iter().find(|(k, _)| *k == &keyhash(b"vrf_2")).map(|(_, v)| *v),
            Some(2)
        );
    }

    #[test]
    fn store_block_hashes_after_handle_mint() {
        let mut state = State::new(&StoreConfig::new(false, true));
        let mut block = make_block(100);
        state.handle_mint(&block, Some(b"vrf_1"));
        block.number += 1;
        state.handle_mint(&block, Some(b"vrf_2"));
        block.number += 1;
        state.handle_mint(&block, Some(b"vrf_2"));

        let block_hashes_1 = state.get_block_hashes(&keyhash(b"vrf_1"));
        assert_eq!(block_hashes_1.len(), 1);
        assert_eq!(block_hashes_1[0], block.hash);
        let block_hashes_2 = state.get_block_hashes(&keyhash(b"vrf_2"));
        assert_eq!(block_hashes_2.len(), 2);
        assert_eq!(block_hashes_2[0], block.hash);
        assert_eq!(block_hashes_2[1], block.hash);
    }

    #[test]
    fn handle_fees_counts_fees() {
        let mut state = State::new(&StoreConfig::default());
        let mut block = make_block(100);

        state.handle_fees(&block, 100);
        block.number += 1;
        state.handle_fees(&block, 250);

        assert_eq!(state.epoch_fees, 350);
    }

    #[test]
    fn end_epoch_resets_and_returns_message() {
        let mut state = State::new(&StoreConfig::default());
        let block = make_block(1);
        state.handle_mint(&block, Some(b"vrf_1"));
        state.handle_fees(&block, 123);

        // Check the message returned
        let ea = state.end_epoch(&block);
        assert_eq!(ea.epoch, 0);
        assert_eq!(ea.total_blocks, 1);
        assert_eq!(ea.total_fees, 123);
        assert_eq!(ea.vrf_vkey_hashes.len(), 1);
        assert_eq!(
            ea.vrf_vkey_hashes.iter().find(|(k, _)| k == &keyhash(b"vrf_1")).map(|(_, v)| *v),
            Some(1)
        );

        // State must be reset
        assert_eq!(state.epoch, 1);
        assert_eq!(state.epoch_blocks, 0);
        assert_eq!(state.epoch_fees, 0);
        assert!(state.blocks_minted.is_empty());
    }

    #[tokio::test]
    async fn state_is_rolled_back() {
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "epoch_activity_counter",
            StateHistoryStore::default_block_store(),
        )));
        let mut state = history.lock().await.get_current_state();
        let mut block = make_block(1);
        state.handle_mint(&block, Some(b"vrf_1"));
        state.handle_fees(&block, 123);
        history.lock().await.commit(block.number, state);

        let mut state = history.lock().await.get_current_state();
        block.number += 1;
        state.handle_mint(&block, Some(b"vrf_1"));
        state.handle_fees(&block, 123);
        history.lock().await.commit(block.number, state);

        let mut state = history.lock().await.get_current_state();
        block = make_block(2);
        let _ = state.end_epoch(&block);
        state.handle_mint(&block, Some(b"vrf_1"));
        state.handle_fees(&block, 123);
        history.lock().await.commit(block.number, state);

        let state = history.lock().await.get_current_state();
        assert_eq!(state.epoch_blocks, 1);
        assert_eq!(state.epoch_fees, 123);
        assert_eq!(
            1,
            state.get_blocks_minted_by_pools(&vec![keyhash(b"vrf_1")])[0]
        );
        assert_eq!(
            3,
            state.get_total_blocks_minted_by_pools(&vec![keyhash(b"vrf_1")])[0]
        );

        // roll back of epoch 2
        block = make_rolled_back_block(2);
        let mut state = history.lock().await.get_rolled_back_state(block.number);
        let _ = state.end_epoch(&block);
        state.handle_mint(&block, Some(b"vrf_2"));
        state.handle_fees(&block, 123);
        history.lock().await.commit(block.number, state);

        let state = history.lock().await.get_current_state();
        assert_eq!(
            2,
            state.get_total_blocks_minted_by_pools(&vec![keyhash(b"vrf_1")])[0]
        );
    }
}
