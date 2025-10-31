use acropolis_common::PoolRetirement;
use acropolis_common::{BlockInfo, PoolId};
use dashmap::DashMap;
use std::sync::Arc;

use crate::store_config::StoreConfig;

#[derive(Debug, Clone)]
pub struct RetiredPoolsHistoryState {
    /// keyed by epoch
    retired_pools_history: Option<Arc<DashMap<u64, Vec<PoolId>>>>,
}

impl RetiredPoolsHistoryState {
    pub fn new(store_config: StoreConfig) -> Self {
        Self {
            retired_pools_history: if store_config.store_retired_pools {
                Some(Arc::new(DashMap::new()))
            } else {
                None
            },
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.retired_pools_history.is_some()
    }

    /// Get Pool History by SPO
    /// Get pools that have been retired so far
    pub fn get_retired_pools(&self) -> Vec<PoolRetirement> {
        self.retired_pools_history
            .as_ref()
            .map(|retired_pools_history| {
                retired_pools_history
                    .iter()
                    .flat_map(|entry| {
                        let epoch = *entry.key();
                        entry
                            .value()
                            .iter()
                            .map(move |pool| PoolRetirement {
                                operator: pool.clone(),
                                epoch,
                            })
                            .collect::<Vec<PoolRetirement>>()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Handle Retired SPOs
    /// Update retired_pools_history with deregistrations
    ///
    pub fn handle_deregistrations(&self, block: &BlockInfo, retired_spos: &[PoolId]) {
        let Some(retired_pools_history) = self.retired_pools_history.as_ref() else {
            return;
        };

        retired_pools_history.insert(block.epoch, retired_spos.to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn retired_pools_history_is_none_when_store_retired_pools_is_false() {
        let state = RetiredPoolsHistoryState::new(default_store_config());
        assert!(state.retired_pools_history.is_none());
    }

    #[test]
    fn retired_pools_history_is_some_when_store_retired_pools_is_true() {
        let state = RetiredPoolsHistoryState::new(save_retired_pools_store_config());
        assert!(state.retired_pools_history.is_some());
    }

    #[test]
    fn get_retired_pools_return_empty() {
        let state = RetiredPoolsHistoryState::new(save_retired_pools_store_config());
        assert_eq!(0, state.get_retired_pools().len());
    }

    #[test]
    fn get_retired_pools_return_data() {
        let state = RetiredPoolsHistoryState::new(save_retired_pools_store_config());

        let block = new_block(2);
        let spo_1 = [1; 28].into();
        let spo_2 = [2; 28].into();
        let retired_spos = vec![spo_1, spo_2];
        state.handle_deregistrations(&block, &retired_spos);

        let retired_pools = state.get_retired_pools();
        assert_eq!(2, retired_pools.len());
        assert_eq!(2, retired_pools[0].epoch);
        assert_eq!(2, retired_pools[1].epoch);
        assert_eq!(spo_1, retired_pools[0].operator);
        assert_eq!(spo_2, retired_pools[1].operator);
    }
}
