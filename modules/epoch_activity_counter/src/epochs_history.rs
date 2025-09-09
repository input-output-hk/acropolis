use acropolis_common::messages::EpochActivityMessage;
use acropolis_common::BlockInfo;
use anyhow::Result;
use dashmap::DashMap;
use std::sync::Arc;

use crate::store_config::StoreConfig;

#[derive(Debug, Clone)]
pub struct EpochsHistoryState {
    epochs_history: Option<Arc<DashMap<u64, EpochActivityMessage>>>,
}

impl EpochsHistoryState {
    pub fn new(store_config: &StoreConfig) -> Self {
        Self {
            epochs_history: if store_config.store_history {
                Some(Arc::new(DashMap::new()))
            } else {
                None
            },
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.epochs_history.is_some()
    }

    /// Get Epoch Activity Message for certain pool operator at certain epoch
    pub fn get_historical_epoch(&self, epoch: u64) -> Result<Option<EpochActivityMessage>> {
        if self.is_enabled() {
            Ok(self
                .epochs_history
                .as_ref()
                .and_then(|epochs| epochs.get(&epoch).map(|e| e.clone())))
        } else {
            Err(anyhow::anyhow!("Historical epoch storage is disabled"))
        }
    }

    /// Handle Epoch Activity
    pub fn handle_epoch_activity(
        &self,
        _block_info: &BlockInfo,
        epoch_activity_message: &EpochActivityMessage,
    ) {
        let Some(epochs_history) = self.epochs_history.as_ref() else {
            return;
        };
        let EpochActivityMessage { epoch, .. } = epoch_activity_message;
        epochs_history.insert(*epoch, epoch_activity_message.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{BlockStatus, Era};

    fn make_block(epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 99,
            number: 42,
            hash: Vec::new(),
            epoch,
            new_epoch: false,
            era: Era::Conway,
            epoch_slot: 0,
            timestamp: 0,
        }
    }

    #[test]
    fn epochs_history_is_none_when_store_history_is_false() {
        let epochs_history = EpochsHistoryState::new(&StoreConfig::new(false));
        assert!(epochs_history.epochs_history.is_none());
    }

    #[test]
    fn epochs_history_is_some_when_store_history_is_true() {
        let epochs_history = EpochsHistoryState::new(&StoreConfig::new(true));
        assert!(epochs_history.epochs_history.is_some());
    }

    #[test]
    fn handle_epoch_activity_saves_history() {
        let epochs_history = EpochsHistoryState::new(&StoreConfig::new(true));
        let block = make_block(200);
        epochs_history.handle_epoch_activity(
            &block,
            &EpochActivityMessage {
                epoch: 199,
                total_blocks: 1,
                total_fees: 50,
                vrf_vkey_hashes: vec![],
            },
        );

        // Use the public API method
        let history = epochs_history
            .get_historical_epoch(199)
            .expect("history disabled in test")
            .expect("epoch history missing");
        assert_eq!(history.total_blocks, 1);
        assert_eq!(history.total_fees, 50);
    }
}
