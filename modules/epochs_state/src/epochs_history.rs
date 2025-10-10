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

    /// Get Epoch Activity Message for certain pool operator at certain epoch
    pub fn get_historical_epoch(&self, epoch: u64) -> Result<Option<EpochActivityMessage>> {
        if let Some(epochs_history) = self.epochs_history.as_ref() {
            Ok(epochs_history.get(&epoch).map(|e| e.clone()))
        } else {
            Err(anyhow::anyhow!("Historical epoch storage is disabled"))
        }
    }

    /// Get Epoch Activity Messages for epochs following a specific epoch. (exclusive)
    pub fn get_next_epochs(&self, epoch: u64) -> Result<Vec<EpochActivityMessage>> {
        if let Some(epochs_history) = self.epochs_history.as_ref() {
            let mut epochs: Vec<EpochActivityMessage> = epochs_history
                .iter()
                .filter(|entry| *entry.key() > epoch)
                .map(|e| e.value().clone())
                .collect();
            epochs.sort_by(|a, b| a.epoch.cmp(&b.epoch));
            Ok(epochs)
        } else {
            Err(anyhow::anyhow!("Historical epoch storage is disabled"))
        }
    }

    /// Get Epoch Activity Messages for epochs following a specific epoch. (exclusive)
    pub fn get_previous_epochs(&self, epoch: u64) -> Result<Vec<EpochActivityMessage>> {
        if let Some(epochs_history) = self.epochs_history.as_ref() {
            let mut epochs: Vec<EpochActivityMessage> = epochs_history
                .iter()
                .filter(|entry| *entry.key() < epoch)
                .map(|e| e.value().clone())
                .collect();
            epochs.sort_by(|a, b| a.epoch.cmp(&b.epoch));
            Ok(epochs)
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
    use acropolis_common::{BlockHash, BlockStatus, Era};

    fn make_block(epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 99,
            number: 42,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: 99,
            new_epoch: false,
            timestamp: 99999,
            era: Era::Conway,
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
                epoch_start_time: 0,
                epoch_end_time: 0,
                first_block_time: 0,
                last_block_time: 0,
                total_blocks: 1,
                total_txs: 1,
                total_outputs: 100,
                total_fees: 50,
                vrf_vkey_hashes: vec![],
                nonce: None,
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

    #[test]
    fn get_next_previous_epochs_sorts_epochs() {
        let epochs_history = EpochsHistoryState::new(&StoreConfig::new(true));
        let block = make_block(200);
        epochs_history.handle_epoch_activity(
            &block,
            &EpochActivityMessage {
                epoch: 199,
                epoch_start_time: 0,
                epoch_end_time: 0,
                first_block_time: 0,
                last_block_time: 0,
                total_blocks: 1,
                total_txs: 1,
                total_outputs: 100,
                total_fees: 50,
                vrf_vkey_hashes: vec![],
                nonce: None,
            },
        );

        let block = make_block(201);
        epochs_history.handle_epoch_activity(
            &block,
            &EpochActivityMessage {
                epoch: 200,
                epoch_start_time: 0,
                epoch_end_time: 0,
                first_block_time: 0,
                last_block_time: 0,
                total_blocks: 1,
                total_txs: 1,
                total_outputs: 100,
                total_fees: 50,
                vrf_vkey_hashes: vec![],
                nonce: None,
            },
        );

        let next_epochs = epochs_history.get_next_epochs(199).expect("history disabled in test");
        assert_eq!(next_epochs.len(), 1);
        assert_eq!(next_epochs[0].epoch, 200);

        let previous_epochs =
            epochs_history.get_previous_epochs(201).expect("history disabled in test");
        assert_eq!(previous_epochs.len(), 2);
        assert_eq!(previous_epochs[0].epoch, 199);

        let next_epochs = epochs_history.get_next_epochs(200).expect("history disabled in test");
        assert_eq!(next_epochs.len(), 0);

        let previous_epochs =
            epochs_history.get_previous_epochs(199).expect("history disabled in test");
        assert_eq!(previous_epochs.len(), 0);
    }
}
