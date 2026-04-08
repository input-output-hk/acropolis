use acropolis_common::{DelegatedStake, PoolId};
use imbl::{HashMap, OrdSet};
use tracing::info;

#[derive(Clone, Default, serde::Serialize)]
pub struct State {
    spdd_history: HashMap<PoolId, DelegatedStake>,
}

impl State {
    pub fn new() -> Self {
        Self {
            spdd_history: HashMap::new(),
        }
    }

    pub fn apply_spdd_snapshot<I>(&mut self, snapshot: I)
    where
        I: IntoIterator<Item = (PoolId, DelegatedStake)>,
    {
        let mut next = self.spdd_history.clone();

        let mut present = OrdSet::new();
        for (k, v_new) in snapshot {
            let changed = match next.get(&k) {
                Some(v_old) => *v_old != v_new,
                None => true,
            };
            if changed {
                next.insert(k, v_new);
            }
            present.insert(k);
        }

        next.retain(|k, _| present.contains(k));

        self.spdd_history = next;
    }

    pub fn get_latest(&self) -> &HashMap<PoolId, DelegatedStake> {
        &self.spdd_history
    }

    // Since this is active stakes
    // we plus 2 to epoch number
    pub fn get_total_active_stakes(&self) -> u64 {
        self.spdd_history.values().map(|v| v.active).sum()
    }

    pub fn tick(&self, num_epochs: usize) {
        let spo_count = self.spdd_history.len();
        info!(
            num_epochs,
            spo_count, "Tracking {num_epochs} epochs, latest snapshot has {spo_count} SPOs"
        );
    }
}
