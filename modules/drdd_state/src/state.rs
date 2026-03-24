use acropolis_common::{DRepCredential, Epoch};
use imbl::{OrdMap, OrdSet};
use tracing::info;

#[derive(Clone, Default)]
pub struct State {
    drdd_history: OrdMap<Epoch, DRepDistribution>,
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
            drdd_history: OrdMap::new(),
        }
    }

    pub fn apply_drdd_snapshot<I>(
        &mut self,
        epoch: Epoch,
        snapshot_dreps: I,
        abstain: u64,
        no_confidence: u64,
    ) where
        I: IntoIterator<Item = (DRepCredential, u64)>,
    {
        let mut next = if epoch == 0 {
            Default::default()
        } else {
            self.drdd_history.get(&(epoch - 1)).cloned().unwrap_or_default()
        };

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

        next.dreps = next.dreps.into_iter().filter(|(k, _)| present.contains(k)).collect();

        self.drdd_history.insert(epoch, next);
    }

    pub fn get_latest(&self) -> Option<&DRepDistribution> {
        self.drdd_history.iter().next_back().map(|(_, v)| v)
    }
    pub fn get_epoch(&self, epoch: Epoch) -> Option<&DRepDistribution> {
        self.drdd_history.get(&epoch)
    }

    pub async fn tick(&self) -> anyhow::Result<()> {
        if let Some((_, latest)) = self.drdd_history.iter().next_back() {
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
