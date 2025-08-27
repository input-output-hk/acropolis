use acropolis_common::{
    state_history::{HistoryKind, StateHistory},
    DRepCredential,
};
use imbl::{OrdMap, OrdSet};
use tracing::info;

pub struct State {
    drdd_history: StateHistory<DRepDistribution>,
}

#[derive(Clone, Default)]
pub struct DRepDistribution {
    pub dreps: OrdMap<DRepCredential, u64>,
    pub abstain: u64,
    pub no_confidence: u64,
}

impl State {
    pub fn new() -> Self {
        Self {
            drdd_history: StateHistory::new("drdd", HistoryKind::EpochState),
        }
    }

    pub fn apply_drdd_snapshot<I>(
        &mut self,
        epoch: u64,
        snapshot_dreps: I,
        abstain: u64,
        no_confidence: u64,
    ) where
        I: IntoIterator<Item = (DRepCredential, u64)>,
    {
        let mut next = self.drdd_history.get_rolled_back_state(epoch);

        next.abstain = abstain;
        next.no_confidence = no_confidence;

        let mut present = OrdSet::new();
        for (k, v_new) in snapshot_dreps {
            let changed = match next.dreps.get(&k) {
                Some(v_old) => *v_old != v_new,
                None => true,
            };
            if changed {
                next.dreps.insert(k.clone(), v_new);
            }
            present.insert(k);
        }

        let to_remove: Vec<_> =
            next.dreps.keys().filter(|k| !present.contains(k)).cloned().collect();
        for k in to_remove {
            next.dreps.remove(&k);
        }

        self.drdd_history.commit(epoch, next);
    }

    pub fn get_latest(&self) -> Option<&DRepDistribution> {
        self.drdd_history.current()
    }
    pub fn get_epoch(&self, epoch: u64) -> Option<&DRepDistribution> {
        self.drdd_history.get_by_index(epoch)
    }

    pub async fn tick(&self) -> anyhow::Result<()> {
        if let Some(latest) = self.drdd_history.current() {
            let drep_count = latest.dreps.len();
            let num_epochs = self.drdd_history.len();
            info!(
                num_epochs,
                drep_count, "Tracking {num_epochs} epochs, latest snapshot has {drep_count} DReps"
            );
        } else {
            info!("DRDD state: no data yet");
        }
        Ok(())
    }
}
