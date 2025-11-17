use crate::{
    immutable_historical_epochs_state::ImmutableHistoricalEpochsState,
    volatile_historical_epochs_state::VolatileHistoricalEpochsState,
};
use acropolis_common::{messages::EpochActivityMessage, BlockInfo};
use anyhow::Result;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct HistoricalEpochsStateConfig {
    pub db_path: String,
}

/// Overall state - stored per epoch
#[derive(Clone)]
pub struct State {
    pub immutable: Arc<ImmutableHistoricalEpochsState>,
    pub volatile: VolatileHistoricalEpochsState,
}

impl State {
    pub fn new(config: &HistoricalEpochsStateConfig) -> Result<Self> {
        let db_path = if Path::new(&config.db_path).is_relative() {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&config.db_path)
        } else {
            PathBuf::from(&config.db_path)
        };

        let immutable = Arc::new(ImmutableHistoricalEpochsState::new(&db_path)?);

        Ok(Self {
            volatile: VolatileHistoricalEpochsState::new(),
            immutable,
        })
    }

    pub async fn prune_volatile(&mut self) {
        let drained = self.volatile.prune_volatile();
        if let Some(ea) = drained {
            self.immutable.update_immutable(ea).await;
        }
    }

    pub fn ready_to_prune(&self, block_info: &BlockInfo) -> bool {
        block_info.epoch > 0
            && Some(block_info.epoch - 1) != self.volatile.last_persisted_epoch
            && block_info.number > self.volatile.block_number + self.volatile.security_param_k
    }

    pub fn get_historical_epoch(&self, epoch: u64) -> Result<Option<EpochActivityMessage>> {
        if let Some(last_persisted_epoch) = self.volatile.last_persisted_epoch {
            if epoch <= last_persisted_epoch {
                return self.immutable.get_historical_epoch(epoch);
            }
        }

        Ok(self.volatile.get_volatile_epoch(epoch))
    }

    pub fn get_next_epochs(&self, epoch: u64) -> Result<Vec<EpochActivityMessage>> {
        let mut epochs = vec![];
        let immutable_epochs_range =
            self.volatile.last_persisted_epoch.and_then(|last_persisted_epoch| {
                if last_persisted_epoch > epoch {
                    Some(epoch + 1..=last_persisted_epoch)
                } else {
                    None
                }
            });

        if let Some(immutable_epochs_range) = immutable_epochs_range {
            epochs.extend(self.immutable.get_epochs(immutable_epochs_range)?);
        }

        if let Some(volatile_ea) = self.volatile.volatile_ea.as_ref() {
            if volatile_ea.epoch > epoch {
                epochs.push(volatile_ea.clone());
            }
        }
        epochs.sort_by(|a, b| a.epoch.cmp(&b.epoch));
        Ok(epochs)
    }

    pub fn get_previous_epochs(&self, epoch: u64) -> Result<Vec<EpochActivityMessage>> {
        let mut epochs = vec![];
        let immutable_epochs_range = self
            .volatile
            .last_persisted_epoch
            .map(|last_persisted_epoch| 0..=last_persisted_epoch.min(epoch - 1));

        if let Some(immutable_epochs_range) = immutable_epochs_range {
            epochs.extend(self.immutable.get_epochs(immutable_epochs_range)?);
        }

        if let Some(volatile_ea) = self.volatile.volatile_ea.as_ref() {
            if volatile_ea.epoch < epoch {
                epochs.push(volatile_ea.clone());
            }
        }
        epochs.sort_by(|a, b| a.epoch.cmp(&b.epoch));
        Ok(epochs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{BlockHash, BlockStatus, Era, PoolId};

    fn make_ea(epoch: u64) -> EpochActivityMessage {
        EpochActivityMessage {
            epoch,
            epoch_start_time: epoch * 10,
            epoch_end_time: epoch * 10 + 10,
            first_block_time: epoch * 10,
            first_block_height: epoch * 10,
            last_block_time: epoch * 10 + 100,
            last_block_height: epoch * 10 + 100,
            total_blocks: 100,
            total_txs: 100,
            total_outputs: 100000,
            total_fees: 10000,
            spo_blocks: vec![(PoolId::default(), 100)],
            nonce: None,
        }
    }

    fn make_block_info(epoch: u64, new_epoch: bool) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 0,
            hash: BlockHash::default(),
            epoch_slot: 0,
            new_epoch,
            era: Era::Shelley,
            number: epoch * 10 + 100,
            epoch,
            timestamp: epoch * 10,
        }
    }

    #[test]
    fn test_get_historical_epoch() {
        let config = HistoricalEpochsStateConfig {
            db_path: "test_db".to_string(),
        };
        let mut state = State::new(&config).unwrap();

        let block_info = make_block_info(1, true);
        let ea = make_ea(0);
        state.volatile.handle_new_epoch(&block_info, &ea);

        let historical_epoch = state.get_historical_epoch(0).unwrap().unwrap();
        assert_eq!(historical_epoch, ea);

        let next_epochs = state.get_next_epochs(0).unwrap();
        assert_eq!(next_epochs, vec![]);

        let previous_epochs = state.get_previous_epochs(1).unwrap();
        assert_eq!(previous_epochs, vec![ea.clone()]);
    }

    #[tokio::test]
    async fn test_persist_epochs() {
        let config = HistoricalEpochsStateConfig {
            db_path: "test_db".to_string(),
        };
        let mut state = State::new(&config).unwrap();

        let block_info = make_block_info(1, true);
        let ea_0 = make_ea(0);
        state.volatile.handle_new_epoch(&block_info, &ea_0);
        let mut block_info = make_block_info(1, false);
        block_info.number += 1;
        assert!(state.ready_to_prune(&block_info));

        state.prune_volatile().await;
        state.immutable.persist_epoch(0).await.unwrap();

        let block_info = make_block_info(2, true);
        let ea_1 = make_ea(1);
        state.volatile.handle_new_epoch(&block_info, &ea_1);
        state.volatile.update_k(20);
        let mut block_info = make_block_info(2, false);
        block_info.number += 1;
        assert!(!state.ready_to_prune(&block_info));
        block_info.number += 20;
        assert!(state.ready_to_prune(&block_info));

        state.prune_volatile().await;
        state.immutable.persist_epoch(1).await.unwrap();

        let historical_epoch = state.immutable.get_historical_epoch(0).unwrap().unwrap();
        assert_eq!(historical_epoch, ea_0);
        let historical_epoch = state.immutable.get_historical_epoch(1).unwrap().unwrap();
        assert_eq!(historical_epoch, ea_1);

        let epochs = state.immutable.get_epochs(0..=1).unwrap();
        assert_eq!(epochs, vec![ea_0, ea_1]);
    }
}
