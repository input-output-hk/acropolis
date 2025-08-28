use acropolis_common::messages::EpochActivityMessage;
use acropolis_common::messages::SPORewardsMessage;
use acropolis_common::messages::SPOStakeDistributionMessage;
use acropolis_common::rational_number::RationalNumber;
use acropolis_common::BlockInfo;
use acropolis_common::KeyHash;
use acropolis_common::PoolEpochState;
use dashmap::DashMap;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::error;

use crate::state_config::StateConfig;

/// Epoch State for certain pool
/// Store active_stake, delegators_count, rewards
///
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochState {
    /// epoch number N
    epoch: u64,
    /// blocks minted during the epoch
    blocks_minted: Option<u64>,
    /// active stake of the epoch N (taken boundary from epoch N-2 to N-1)
    active_stake: Option<u64>,
    /// active size = active_stake / total_active_stake
    active_size: Option<RationalNumber>,
    /// delegators count by the end of the epoch
    delegators_count: Option<u64>,
    /// Total rewards pool has received during epoch
    pool_reward: Option<u64>,
    /// pool's operator's reward
    spo_reward: Option<u64>,
}

impl EpochState {
    fn new(epoch: u64) -> Self {
        Self {
            epoch,
            blocks_minted: None,
            active_stake: None,
            active_size: None,
            delegators_count: None,
            pool_reward: None,
            spo_reward: None,
        }
    }

    fn to_pool_epoch_state(&self) -> PoolEpochState {
        PoolEpochState {
            epoch: self.epoch,
            blocks_minted: self.blocks_minted.unwrap_or(0),
            active_stake: self.active_stake.unwrap_or(0),
            active_size: self.active_size.unwrap_or(RationalNumber::from(0)),
            delegators_count: self.delegators_count.unwrap_or(0),
            pool_reward: self.pool_reward.unwrap_or(0),
            spo_reward: self.spo_reward.unwrap_or(0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EpochsHistoryState {
    epochs_history: Option<Arc<DashMap<KeyHash, BTreeMap<u64, EpochState>>>>,
}

impl EpochsHistoryState {
    pub fn new(state_config: StateConfig) -> Self {
        Self {
            epochs_history: if state_config.store_history {
                Some(Arc::new(DashMap::new()))
            } else {
                None
            },
        }
    }

    /// Get Epoch State for certain pool operator at certain epoch
    #[allow(dead_code)]
    pub fn get_epoch_state(&self, spo: &KeyHash, epoch: u64) -> Option<EpochState> {
        self.epochs_history
            .as_ref()
            .and_then(|epochs| epochs.get(spo).and_then(|epochs| epochs.get(&epoch).cloned()))
    }

    /// Get Pool History by SPO
    pub fn get_pool_history(&self, spo: &KeyHash) -> Option<Vec<PoolEpochState>> {
        self.epochs_history
            .as_ref()
            .and_then(|epochs| epochs.get(spo))
            .map(|epochs| epochs.values().map(|state| state.to_pool_epoch_state()).collect())
    }

    /// Handle SPO Stake Distribution
    /// Update epochs_history with active_stake (for spdd_message.epoch + 2)
    ///
    pub fn handle_spdd(&self, _block: &BlockInfo, spdd_message: &SPOStakeDistributionMessage) {
        let Some(epochs_history) = self.epochs_history.as_ref() else {
            return;
        };
        let SPOStakeDistributionMessage { epoch, spos } = spdd_message;
        let epoch_to_update = *epoch + 2;

        let total_active_stake = spos.par_iter().map(|(_, value)| value.active).sum();

        spos.par_iter().for_each(|(spo, value)| {
            Self::update_epochs_history_with(epochs_history, spo, epoch_to_update, |epoch_state| {
                epoch_state.active_stake = Some(value.active);
                epoch_state.delegators_count = Some(value.active_delegators_count);
                if total_active_stake > 0 {
                    epoch_state.active_size =
                        Some(RationalNumber::new(value.active, total_active_stake));
                }
            });
        });
    }

    /// Handle SPO rewards data calculated from accounts-state
    /// NOTE:
    /// The calculated result is one epoch off against blockfrost's response.
    pub fn handle_spo_rewards(&self, block: &BlockInfo, spo_rewards_message: &SPORewardsMessage) {
        let Some(epochs_history) = self.epochs_history.as_ref() else {
            return;
        };
        let SPORewardsMessage { epoch, spos } = spo_rewards_message;
        if *epoch != block.epoch - 1 {
            error!(
                "SPO Rewards Message's epoch {} is wrong against current block's epoch {}",
                *epoch, block.epoch
            )
        }

        // update epochs history if set
        spos.par_iter().for_each(|(spo, value)| {
            Self::update_epochs_history_with(epochs_history, spo, *epoch, |epoch_state| {
                epoch_state.pool_reward = Some(value.total_rewards);
                epoch_state.spo_reward = Some(value.operator_rewards);
            });
        });
    }

    /// Handle Epoch Activity
    pub fn handle_epoch_activity(
        &self,
        _block: &BlockInfo,
        epoch_activity_message: &EpochActivityMessage,
        spos: &Vec<(KeyHash, u64)>,
    ) {
        let Some(epochs_history) = self.epochs_history.as_ref() else {
            return;
        };
        let EpochActivityMessage { epoch, .. } = epoch_activity_message;

        spos.iter().for_each(|(spo, amount)| {
            Self::update_epochs_history_with(epochs_history, &spo, *epoch, |epoch_state| {
                epoch_state.blocks_minted = Some(*amount as u64);
            });
        })
    }

    fn update_epochs_history_with(
        epochs_history: &Arc<DashMap<KeyHash, BTreeMap<u64, EpochState>>>,
        spo: &KeyHash,
        epoch: u64,
        update_fn: impl FnOnce(&mut EpochState),
    ) {
        let mut epochs = epochs_history.entry(spo.clone()).or_insert_with(BTreeMap::new);
        let epoch_state = epochs.entry(epoch).or_insert_with(|| EpochState::new(epoch));
        update_fn(epoch_state);
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::{DelegatedStake, SPORewards};

    use super::*;
    use crate::test_utils::*;

    #[test]
    fn epochs_history_is_none_when_store_history_is_false() {
        let epochs_history = EpochsHistoryState::new(default_state_config());
        assert!(epochs_history.epochs_history.is_none());
    }

    #[test]
    fn epochs_history_is_some_when_store_history_is_true() {
        let epochs_history = EpochsHistoryState::new(save_history_state_config());
        assert!(epochs_history.epochs_history.is_some());
    }

    #[tokio::test]
    async fn get_pool_history_returns_none_when_spo_is_not_found() {
        let epochs_history = EpochsHistoryState::new(save_history_state_config());
        let pool_history = epochs_history.get_pool_history(&vec![1]);
        assert!(pool_history.is_none());
    }

    #[tokio::test]
    async fn get_pool_history_returns_data() {
        let epochs_history = EpochsHistoryState::new(save_history_state_config());

        let block = new_block(2);
        let mut spdd_msg = new_spdd_message(1);
        spdd_msg.spos = vec![(
            vec![1],
            DelegatedStake {
                active: 1,
                active_delegators_count: 1,
                live: 1,
            },
        )];
        epochs_history.handle_spdd(&block, &spdd_msg);

        let mut epoch_activity_msg = new_epoch_activity_message(1);
        epoch_activity_msg.vrf_vkey_hashes = vec![(vec![11], 1)];
        epoch_activity_msg.total_blocks = 1;
        epoch_activity_msg.total_fees = 10;
        epochs_history.handle_epoch_activity(&block, &epoch_activity_msg, &vec![(vec![1], 1)]);

        let mut spo_rewards_msg = new_spo_rewards_message(1);
        spo_rewards_msg.spos = vec![(
            vec![1],
            SPORewards {
                total_rewards: 100,
                operator_rewards: 10,
            },
        )];
        epochs_history.handle_spo_rewards(&block, &spo_rewards_msg);

        let pool_history = epochs_history.get_pool_history(&vec![1]).unwrap();
        assert_eq!(2, pool_history.len());
        let first_epoch = pool_history.get(0).unwrap();
        let third_epoch = pool_history.get(1).unwrap();
        assert_eq!(1, first_epoch.epoch);
        assert_eq!(1, first_epoch.blocks_minted);
        assert_eq!(3, third_epoch.epoch);
        assert_eq!(1, third_epoch.active_stake);
        assert_eq!(RationalNumber::new(1, 1), third_epoch.active_size);
        assert_eq!(1, third_epoch.delegators_count);
        assert_eq!(100, first_epoch.pool_reward);
        assert_eq!(10, first_epoch.spo_reward);
    }
}
