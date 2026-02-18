use acropolis_common::{messages::AddressDeltasMessage, BlockInfo, Era};

/// Epoch summary emitted by midnight-state logging runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochSummary {
    pub epoch: u64,
    pub era: Era,
    pub blocks: usize,
    pub delta_count: usize,
    pub created_utxos: usize,
    pub spent_utxos: usize,
    pub saw_compact: bool,
}

trait EpochTotalsObserver {
    fn start_block(&mut self, block: &BlockInfo);
    fn observe_deltas(&mut self, deltas: &AddressDeltasMessage);
    fn finalise_block(&mut self, block: &BlockInfo);
}

#[derive(Clone, Default)]
pub struct EpochTotals {
    compact_blocks: usize,
    extended_blocks: usize,
    delta_count: usize,
    created_utxos: usize,
    spent_utxos: usize,
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
    pub fn start_block(&mut self, block: &BlockInfo) {
        <Self as EpochTotalsObserver>::start_block(self, block);
    }

    pub fn observe_deltas(&mut self, deltas: &AddressDeltasMessage) {
        <Self as EpochTotalsObserver>::observe_deltas(self, deltas);
    }

    pub fn finalise_block(&mut self, block: &BlockInfo) {
        <Self as EpochTotalsObserver>::finalise_block(self, block);
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
            blocks: self.extended_blocks,
            delta_count: self.delta_count,
            created_utxos: self.created_utxos,
            spent_utxos: self.spent_utxos,
            saw_compact: self.compact_blocks > 0,
        }
    }

    pub fn reset_epoch(&mut self) {
        self.compact_blocks = 0;
        self.extended_blocks = 0;
        self.delta_count = 0;
        self.created_utxos = 0;
        self.spent_utxos = 0;
        self.last_checkpoint = None;
    }
}

impl EpochTotalsObserver for EpochTotals {
    fn start_block(&mut self, _block: &BlockInfo) {}

    fn observe_deltas(&mut self, deltas: &AddressDeltasMessage) {
        match deltas {
            AddressDeltasMessage::Deltas(compact_deltas) => {
                self.compact_blocks += 1;
                self.delta_count += compact_deltas.len();
                self.created_utxos +=
                    compact_deltas.iter().map(|delta| delta.created_utxos.len()).sum::<usize>();
                self.spent_utxos +=
                    compact_deltas.iter().map(|delta| delta.spent_utxos.len()).sum::<usize>();
            }
            AddressDeltasMessage::ExtendedDeltas(extended_deltas) => {
                self.extended_blocks += 1;
                self.delta_count += extended_deltas.len();
                self.created_utxos +=
                    extended_deltas.iter().map(|delta| delta.created_utxos.len()).sum::<usize>();
                self.spent_utxos +=
                    extended_deltas.iter().map(|delta| delta.spent_utxos.len()).sum::<usize>();
            }
        }
    }

    fn finalise_block(&mut self, block: &BlockInfo) {
        self.last_checkpoint = Some(EpochCheckpoint::from_block(block));
    }
}
