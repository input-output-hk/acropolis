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
    pub async fn new(config: &HistoricalEpochsStateConfig) -> Result<Self> {
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
