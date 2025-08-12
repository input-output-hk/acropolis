use acropolis_common::{DelegatedStake, KeyHash};
use imbl::{OrdMap, OrdSet};
use tracing::info;

pub struct State {
    historical_distributions: OrdMap<u64, OrdMap<KeyHash, DelegatedStake>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            historical_distributions: OrdMap::new(),
        }
    }

    pub fn apply_spdd_snapshot<I>(&mut self, epoch: u64, snapshot: I)
    where
        I: IntoIterator<Item = (KeyHash, DelegatedStake)>,
    {
        let mut next = self.get_latest().cloned().unwrap_or_else(OrdMap::new);

        // Update new or changed entries
        let mut present = OrdSet::new();
        for (k, v_new) in snapshot {
            let changed = match next.get(&k) {
                Some(v_old) => *v_old != v_new,
                None => true,
            };
            if changed {
                next.insert(k.clone(), v_new);
            }
            present.insert(k);
        }

        // Remove keys that disappeared.
        let to_remove: Vec<_> =
            next.keys().filter(|k| !present.contains::<[u8]>((**k).as_slice())).cloned().collect();
        for k in to_remove {
            next.remove(&k);
        }

        self.insert_spdd(epoch, next);
    }

    pub fn insert_spdd(&mut self, epoch: u64, spdd: OrdMap<KeyHash, DelegatedStake>) {
        self.historical_distributions.insert(epoch, spdd);
    }

    pub fn get_latest(&self) -> Option<&OrdMap<KeyHash, DelegatedStake>> {
        self.historical_distributions.iter().next_back().map(|(_, map)| map)
    }

    pub fn get_epoch(&self, epoch: u64) -> Option<&OrdMap<KeyHash, DelegatedStake>> {
        self.historical_distributions.get(&epoch)
    }

    pub async fn tick(&self) -> anyhow::Result<()> {
        let num_epochs = self.historical_distributions.len();
        let latest = self.historical_distributions.iter().next_back();

        if let Some((epoch, spo_map)) = latest {
            let spo_count = spo_map.len();
            info!(
                num_epochs,
                latest_epoch = *epoch,
                spo_count,
                "Tracking {num_epochs} epochs, latest is {epoch} with {spo_count} SPOs"
            );
        } else {
            info!("SPDD state: no data yet");
        }

        Ok(())
    }
}
