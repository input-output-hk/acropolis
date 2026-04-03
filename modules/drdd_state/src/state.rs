use acropolis_common::DRepCredential;
use imbl::{OrdMap, OrdSet};
use tracing::info;

#[derive(Clone, Default, serde::Serialize)]
pub struct State {
    drdd_history: DRepDistribution,
}

#[derive(Clone, Default, serde::Serialize)]
pub struct DRepDistribution {
    pub dreps: OrdMap<DRepCredential, u64>,
    pub abstain: u64,
    pub no_confidence: u64,
}

impl State {
    pub fn new() -> Self {
        Self {
            drdd_history: DRepDistribution::default(),
        }
    }

    pub fn apply_drdd_snapshot<I>(&mut self, snapshot_dreps: I, abstain: u64, no_confidence: u64)
    where
        I: IntoIterator<Item = (DRepCredential, u64)>,
    {
        let mut next = self.drdd_history.clone();

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

        self.drdd_history = next;
    }

    pub fn get_latest(&self) -> &DRepDistribution {
        &self.drdd_history
    }

    pub fn tick(&self, num_epochs: usize) {
        let drep_count = self.drdd_history.dreps.len();
        info!(
            num_epochs,
            drep_count, "Tracking {num_epochs} epochs, latest snapshot has {drep_count} DReps"
        );
    }
}
