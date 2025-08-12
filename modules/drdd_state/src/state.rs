use acropolis_common::DRepCredential;
use imbl::{OrdMap, OrdSet};
use tracing::info;

pub struct State {
    historical_distributions: OrdMap<u64, DRepDistribution>,
}

#[derive(Clone)]
pub struct DRepDistribution {
    pub dreps: OrdMap<DRepCredential, u64>,
    pub abstain: u64,
    pub no_confidence: u64,
}

impl State {
    pub fn new() -> Self {
        Self {
            historical_distributions: OrdMap::new(),
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
        let mut next = self.get_latest().cloned().unwrap_or_else(|| DRepDistribution {
            dreps: OrdMap::new(),
            abstain: 0,
            no_confidence: 0,
        });

        next.abstain = abstain;
        next.no_confidence = no_confidence;

        // Update new or changed entries
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

        // Remove keys that disappeared.
        let to_remove: Vec<_> =
            next.dreps.keys().filter(|k| !present.contains(k)).cloned().collect();
        for k in to_remove {
            next.dreps.remove(&k);
        }

        self.insert_drdd(epoch, next);
    }

    pub fn insert_drdd(&mut self, epoch: u64, drdd: DRepDistribution) {
        self.historical_distributions.insert(epoch, drdd);
    }

    pub fn get_latest(&self) -> Option<&DRepDistribution> {
        self.historical_distributions.iter().next_back().map(|(_, map)| map)
    }
    pub fn get_epoch(&self, epoch: u64) -> Option<&DRepDistribution> {
        self.historical_distributions.get(&epoch)
    }

    pub async fn tick(&self) -> anyhow::Result<()> {
        let num_epochs = self.historical_distributions.len();
        let latest = self.historical_distributions.iter().next_back();

        if let Some((epoch, drep_map)) = latest {
            let drep_count = drep_map.dreps.len();
            info!(
                num_epochs,
                latest_epoch = *epoch,
                drep_count,
                "Tracking {num_epochs} epochs, latest is {epoch} with {drep_count} DReps"
            );
        } else {
            info!("DRDD state: no data yet");
        }

        Ok(())
    }
}
