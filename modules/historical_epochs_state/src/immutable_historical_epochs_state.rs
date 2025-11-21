use std::{collections::VecDeque, path::Path};

use acropolis_common::messages::EpochActivityMessage;
use anyhow::Result;
use fjall::{Keyspace, Partition, PartitionCreateOptions, PersistMode};
use minicbor::{decode, to_vec};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

pub struct ImmutableHistoricalEpochsState {
    epochs_history: Partition,
    keyspace: Keyspace,
    pub pending: Mutex<VecDeque<EpochActivityMessage>>,
    pub max_pending: usize,
}

impl ImmutableHistoricalEpochsState {
    pub fn new(path: impl AsRef<Path>, clear_on_start: bool) -> Result<Self> {
        let path = path.as_ref();
        if clear_on_start && path.exists() {
            std::fs::remove_dir_all(path)?;
        }

        let cfg = fjall::Config::new(path)
            // 4MB write buffer since EpochActivityMessage is not that big
            .max_write_buffer_size(4 * 1024 * 1024)
            // Enable manual control of flushing
            // We store EpochActivityMessage only every 5 days (need manual Flush)
            .manual_journal_persist(true);
        let keyspace = Keyspace::open(cfg)?;

        let epochs_history =
            keyspace.open_partition("epochs_history", PartitionCreateOptions::default())?;

        Ok(Self {
            epochs_history,
            keyspace,
            pending: Mutex::new(VecDeque::new()),
            max_pending: 5,
        })
    }

    pub async fn update_immutable(&self, ea: EpochActivityMessage) {
        let mut pending = self.pending.lock().await;
        if pending.len() >= self.max_pending {
            warn!("historical epochs state pending buffer full, dropping oldest");
            pending.pop_front();
        }
        pending.push_back(ea);
    }

    /// Persists pending EpochActivityMessages for Epoch N - 1
    /// at the first block of Epoch N
    /// There should be only one EpochActivityMessage for each epoch
    /// Returns the number of persisted EpochActivityMessages
    /// Errors if the batch commit or persist fails
    pub async fn persist_epoch(&self, epoch: u64) -> Result<u32> {
        let saving_epoch = epoch - 1;
        let drained_epochs = {
            let mut pending = self.pending.lock().await;
            std::mem::take(&mut *pending)
        };

        let mut batch = self.keyspace.batch();
        let mut persisted_epochs: u32 = 0;

        for ea in drained_epochs {
            let epoch_key = Self::make_epoch_key(ea.epoch);
            batch.insert(&self.epochs_history, epoch_key, to_vec(&ea)?);
            persisted_epochs += 1;
        }

        if let Err(e) = batch.commit() {
            error!("batch commit failed for epoch {saving_epoch}: {e}");
            return Err(e.into());
        }

        if let Err(e) = self.keyspace.persist(PersistMode::Buffer) {
            error!("persist failed for epoch {saving_epoch}: {e}");
            return Err(e.into());
        }

        info!("persisted {persisted_epochs} epochs for epoch {saving_epoch}");
        Ok(persisted_epochs)
    }

    pub fn get_historical_epoch(&self, epoch: u64) -> Result<Option<EpochActivityMessage>> {
        let epoch_key = Self::make_epoch_key(epoch);
        let slice = self.epochs_history.get(epoch_key)?;
        if let Some(slice) = slice.as_ref() {
            let decoded: EpochActivityMessage = decode(slice)?;
            Ok(Some(decoded))
        } else {
            Ok(None)
        }
    }

    pub fn get_epochs(
        &self,
        range: std::ops::RangeInclusive<u64>,
    ) -> Result<Vec<EpochActivityMessage>> {
        let mut epochs = Vec::new();
        let start_key = Self::make_epoch_key(*range.start());
        let end_key = Self::make_epoch_key(*range.end());

        for result in self.epochs_history.range(start_key..=end_key) {
            let (_, slice) = result?;
            let decoded: EpochActivityMessage = decode(&slice)?;
            epochs.push(decoded);
        }
        Ok(epochs)
    }

    fn make_epoch_key(epoch: u64) -> [u8; 8] {
        epoch.to_be_bytes()
    }
}
