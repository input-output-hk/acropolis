use std::{collections::BTreeMap, sync::Arc};

use acropolis_common::{messages::SPOStakeDistributionMessage, BlockInfo, KeyHash};
use dashmap::DashMap;
use rayon::prelude::*;
use serde::Serialize;
use tracing::error;

// Aggregated SPO State by epoch N-1 (when current epoch is N)
// Active Stakes and total blocks minted count
#[derive(Clone)]
pub struct AggregatedSPOState {
    /// Active stakes for each pool operator
    /// (epoch number, active stake)
    /// Remove elements when epoch number is less than current epoch number
    pub active_stakes: Arc<DashMap<KeyHash, BTreeMap<u64, u64>>>,
}

#[derive(Default, Debug, Clone, Serialize)]
pub struct TotalBlocksMintedState {
    /// block number of Epoch Boundary from N-1 to N
    block: u64,
}

impl AggregatedSPOState {
    pub fn new() -> Self {
        Self {
            active_stakes: Arc::new(DashMap::new()),
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
}
