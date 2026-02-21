use acropolis_common::{BlockInfo, Era};

/// Epoch summary emitted by midnight-state logging runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochSummary {
    pub epoch: u64,
    pub era: Era,
    pub indexed_night_utxos: usize,
}

#[derive(Clone, Default)]
pub struct EpochTotals {
    indexed_night_utxos: usize,
    last_checkpoint: Option<EpochCheckpoint>,
}

#[derive(Clone)]
struct EpochCheckpoint {
    epoch: u64,
    era: Era,
}

impl EpochCheckpoint {
    fn from_block(block: &BlockInfo) -> Self {
        Self {
            epoch: block.epoch,
            era: block.era,
        }
    }
}

impl EpochTotals {
    pub fn add_indexed_night_utxos(&mut self, count: usize) {
        self.indexed_night_utxos += count;
    }

    pub fn finalise_block(&mut self, block: &BlockInfo) {
        self.last_checkpoint = Some(EpochCheckpoint::from_block(block));
    }

    pub fn summarise_completed_epoch(&self, boundary_block: &BlockInfo) -> EpochSummary {
        let (epoch, era) = if let Some(checkpoint) = self.last_checkpoint.as_ref() {
            (checkpoint.epoch, checkpoint.era)
        } else {
            (boundary_block.epoch.saturating_sub(1), boundary_block.era)
        };

        EpochSummary {
            epoch,
            era,
            indexed_night_utxos: self.indexed_night_utxos,
        }
    }

    pub fn reset_epoch(&mut self) {
        self.indexed_night_utxos = 0;
        self.last_checkpoint = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{BlockHash, BlockIntent, BlockStatus};

    fn mk_block(number: u64, epoch: u64, era: Era) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
            slot: number,
            number,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: number,
            new_epoch: false,
            is_new_era: false,
            tip_slot: None,
            timestamp: 0,
            era,
        }
    }

    #[test]
    fn tracks_indexed_night_utxos_for_epoch() {
        let mut totals = EpochTotals::default();
        let block = mk_block(10, 100, Era::Conway);

        totals.add_indexed_night_utxos(2);
        totals.add_indexed_night_utxos(1);
        totals.finalise_block(&block);

        let boundary = mk_block(11, 101, Era::Conway);
        let summary = totals.summarise_completed_epoch(&boundary);
        assert_eq!(summary.epoch, 100);
        assert_eq!(summary.era, Era::Conway);
        assert_eq!(summary.indexed_night_utxos, 3);
    }

    #[test]
    fn summarise_uses_boundary_epoch_when_checkpoint_absent() {
        let mut totals = EpochTotals::default();
        totals.add_indexed_night_utxos(7);

        let boundary = mk_block(99, 501, Era::Conway);
        let summary = totals.summarise_completed_epoch(&boundary);

        assert_eq!(summary.epoch, 500);
        assert_eq!(summary.era, Era::Conway);
        assert_eq!(summary.indexed_night_utxos, 7);
    }
}
