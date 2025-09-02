use std::{collections::BTreeMap, sync::Arc};

use acropolis_common::{
    messages::{EpochActivityMessage, SPOStakeDistributionMessage},
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, KeyHash,
};
use dashmap::DashMap;
use imbl::HashMap;
use rayon::prelude::*;
use serde::Serialize;
use tokio::sync::Mutex;
use tracing::error;

// Aggregated SPO State by epoch N-1 (when current epoch is N)
// Active Stakes and total blocks minted count
#[derive(Clone)]
pub struct AggregatedSPOState {
    /// Active stakes for each pool operator
    /// (epoch number, active stake)
    /// Remove elements when epoch number is less than current epoch number
    pub active_stakes: Arc<DashMap<KeyHash, BTreeMap<u64, u64>>>,

    /// Volatile total blocks minted state, one per epoch
    /// Pop on first element when block number is smaller than `current block - SECURITY_PARAMETER_K`
    pub total_blocks_minted_history: Arc<Mutex<StateHistory<TotalBlocksMintedState>>>,
}

#[derive(Default, Debug, Clone, Serialize)]
pub struct TotalBlocksMintedState {
    /// block number of Epoch Boundary from N-1 to N
    block: u64,
    /// total blocks minted for each pool operator keyed by vrf_key_hash
    /// until the end of Epoch N-1
    total_blocks_minted: HashMap<KeyHash, u64>,
}

impl AggregatedSPOState {
    pub fn new() -> Self {
        Self {
            active_stakes: Arc::new(DashMap::new()),
            total_blocks_minted_history: Arc::new(Mutex::new(StateHistory::new(
                "aggregated-spo-states/total-blocks-minted",
                StateHistoryStore::default_block_store(),
            ))),
        }
    }

    /// Get Pools Active Stakes by epoch and total active stake
    /// ## Arguments
    /// * `pools_operators` - A vector of pool operator hashes
    /// * `epoch` - The epoch to get the active stakes for
    /// ## Returns
    /// `(Vec<u64>, u64)` - a vector of active stakes for each pool operator and the total active stake.
    pub fn get_pools_active_stakes(
        &self,
        pools_operators: &Vec<KeyHash>,
        epoch: u64,
    ) -> (Vec<u64>, u64) {
        let active_stakes = pools_operators
            .par_iter()
            .map(|spo| self.get_active_stake(spo, epoch).unwrap_or(0))
            .collect::<Vec<u64>>();
        let total_active_stake = self.get_total_active_stake(epoch);
        (active_stakes, total_active_stake)
    }

    fn get_active_stake(&self, spo: &KeyHash, epoch: u64) -> Option<u64> {
        self.active_stakes.get(spo).map(|stakes| stakes.get(&epoch).cloned()).flatten()
    }

    fn get_total_active_stake(&self, epoch: u64) -> u64 {
        self.active_stakes.iter().map(|entry| entry.value().get(&epoch).cloned().unwrap_or(0)).sum()
    }

    /// Get total blocks minted for each vrf vkey hash
    /// ## Arguments
    /// * `vrf_key_hashes` - A vector of vrf key hashes
    /// ## Returns
    /// `Vec<u64>` - a vector of total blocks minted for each vrf key hash.
    pub async fn get_total_blocks_minted(&self, vrf_key_hashes: &Vec<KeyHash>) -> Vec<u64> {
        let locked_history = self.total_blocks_minted_history.lock().await;
        let Some(current) = locked_history.current() else {
            return vec![0; vrf_key_hashes.len()];
        };
        let total_blocks_minted = vrf_key_hashes
            .iter()
            .map(|vrf_vkey_hash| {
                current.total_blocks_minted.get(vrf_vkey_hash).cloned().unwrap_or(0)
            })
            .collect();
        total_blocks_minted
    }

    /// Handle SPO Stake Distribution
    /// Live stake snapshots taken at Epoch N - 1 to N boundary (Mark at Epoch N)
    /// Active stake is valid from Epoch N + 1 (Set at Epoch N + 1)
    ///
    pub fn handle_spdd(&self, block: &BlockInfo, spdd_message: &SPOStakeDistributionMessage) {
        let SPOStakeDistributionMessage { epoch, spos } = spdd_message;
        if *epoch != block.epoch - 1 {
            error!(
                "SPO Stake Distribution Message's epoch {} is wrong against current block's epoch {}",
                *epoch, block.epoch
            )
        }
        let epoch_to_update = *epoch + 2;

        // update active stakes
        spos.par_iter().for_each(|(spo, value)| {
            let mut active_stakes = self
                .active_stakes
                .entry(spo.clone())
                .and_modify(|stakes| stakes.retain(|k, _| *k >= block.epoch))
                .or_insert_with(BTreeMap::new);

            active_stakes.insert(epoch_to_update, value.active);
        });
    }

    /// Handle Epoch Activity
    /// Returns blocks minted amount keyed by spo
    ///
    pub async fn handle_epoch_activity(
        &self,
        block: &BlockInfo,
        epoch_activity_message: &EpochActivityMessage,
    ) {
        let EpochActivityMessage {
            epoch,
            vrf_vkey_hashes,
            ..
        } = epoch_activity_message;
        if *epoch != block.epoch - 1 {
            error!(
                "Epoch Activity Message's epoch {} is wrong against current block's epoch {}",
                *epoch, block.epoch
            )
        }

        let mut locked_history = self.total_blocks_minted_history.lock().await;
        let mut total_blocks_minted =
            locked_history.get_rolled_back_state(block.number).total_blocks_minted;

        // handle blocks_minted state
        vrf_vkey_hashes.iter().for_each(|(vrf_vkey_hash, amount)| {
            total_blocks_minted
                .entry(vrf_vkey_hash.clone())
                .and_modify(|v| *v += *amount as u64)
                .or_insert(*amount as u64);
        });

        let new_state = TotalBlocksMintedState {
            block: block.number,
            total_blocks_minted,
        };

        locked_history.commit(block.number, new_state);
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::DelegatedStake;

    use super::*;
    use crate::test_utils::*;

    #[tokio::test]
    async fn new_state_returns_zeros() {
        let aggregated_state = AggregatedSPOState::new();
        assert!(aggregated_state.active_stakes.is_empty());
        let total_blocks_minted =
            aggregated_state.get_total_blocks_minted(&vec![vec![11], vec![12]]).await;
        assert_eq!(2, total_blocks_minted.len());
        assert_eq!(1, total_blocks_minted[0]);
        assert_eq!(2, total_blocks_minted[1]);
    }

    #[test]
    fn active_stakes_not_empty_after_handle_spdd() {
        let aggregated_state = AggregatedSPOState::new();
        let block = new_block(2);
        let mut msg = new_spdd_message(1);
        msg.spos = vec![
            (
                vec![1],
                DelegatedStake {
                    active: 1,
                    active_delegators_count: 1,
                    live: 1,
                },
            ),
            (
                vec![2],
                DelegatedStake {
                    active: 2,
                    active_delegators_count: 2,
                    live: 2,
                },
            ),
        ];
        aggregated_state.handle_spdd(&block, &msg);
        let (active_stakes, total_active_stake) =
            aggregated_state.get_pools_active_stakes(&vec![vec![1], vec![2]], 3);
        assert_eq!(2, active_stakes.len());
        assert_eq!(1, active_stakes[0]);
        assert_eq!(2, active_stakes[1]);
        assert_eq!(3, total_active_stake);
    }

    #[tokio::test]
    async fn total_blocks_minted_not_empty_after_handle_epoch_activity() {
        let aggregated_state = AggregatedSPOState::new();
        let block = new_block(2);
        let mut msg = new_epoch_activity_message(1);
        msg.vrf_vkey_hashes = vec![(vec![11], 1), (vec![12], 2)];
        aggregated_state.handle_epoch_activity(&block, &msg).await;
        let total_blocks_minted =
            aggregated_state.get_total_blocks_minted(&vec![vec![11], vec![12]]).await;
        assert_eq!(2, total_blocks_minted.len());
        assert_eq!(1, total_blocks_minted[0]);
        assert_eq!(2, total_blocks_minted[1]);
    }

    #[tokio::test]
    async fn total_blocks_minted_history_pruned_after_rollback() {
        let aggregated_state = AggregatedSPOState::new();
        let mut block = new_block(2);
        let mut msg = new_epoch_activity_message(1);
        msg.vrf_vkey_hashes = vec![(vec![11], 1), (vec![12], 2)];
        aggregated_state.handle_epoch_activity(&block, &msg).await;
        assert_eq!(
            1,
            aggregated_state.total_blocks_minted_history.lock().await.len()
        );

        block = new_block(3);
        msg = new_epoch_activity_message(2);
        msg.vrf_vkey_hashes = vec![(vec![11], 3), (vec![12], 4)];
        aggregated_state.handle_epoch_activity(&block, &msg).await;
        assert_eq!(
            2,
            aggregated_state.total_blocks_minted_history.lock().await.len()
        );

        block = new_block(4);
        msg = new_epoch_activity_message(3);
        msg.vrf_vkey_hashes = vec![(vec![11], 5), (vec![12], 6)];
        aggregated_state.handle_epoch_activity(&block, &msg).await;
        assert_eq!(
            3,
            aggregated_state.total_blocks_minted_history.lock().await.len()
        );

        block = new_block(2);
        msg = new_epoch_activity_message(1);
        msg.vrf_vkey_hashes = vec![(vec![11], 7), (vec![12], 8)];
        aggregated_state.handle_epoch_activity(&block, &msg).await;
        assert_eq!(
            1,
            aggregated_state.total_blocks_minted_history.lock().await.len()
        );
    }
}
