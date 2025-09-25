use acropolis_common::{
    state_history::{StateHistory, StateHistoryStore},
    DelegatedStake, KeyHash,
};
use imbl::{OrdMap, OrdSet};
use tracing::info;

pub struct State {
    spdd_history: StateHistory<OrdMap<KeyHash, DelegatedStake>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            spdd_history: StateHistory::new("spdd", StateHistoryStore::Unbounded),
        }
    }

    pub fn apply_spdd_snapshot<I>(&mut self, epoch: u64, snapshot: I)
    where
        I: IntoIterator<Item = (KeyHash, DelegatedStake)>,
    {
        let mut next = self.spdd_history.get_rolled_back_state(epoch);

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

        let to_remove: Vec<_> =
            next.keys().filter(|k| !present.contains::<[u8]>((**k).as_slice())).cloned().collect();
        for k in to_remove {
            next.remove(&k);
        }

        self.spdd_history.commit(epoch, next);
    }

    #[allow(dead_code)]
    pub fn get_latest(&self) -> Option<&OrdMap<KeyHash, DelegatedStake>> {
        self.spdd_history.current()
    }

    #[allow(dead_code)]
    pub fn get_epoch(&self, epoch: u64) -> Option<&OrdMap<KeyHash, DelegatedStake>> {
        self.spdd_history.get_by_index(epoch)
    }

    pub fn get_epoch_total_active_stakes(&self, epoch: u64) -> Option<u64> {
        self.spdd_history.get_by_index(epoch).map(|state| state.values().map(|v| v.active).sum())
    }

    pub async fn tick(&self) -> anyhow::Result<()> {
        if let Some(state) = self.spdd_history.current() {
            let spo_count = state.len();
            let num_epochs = self.spdd_history.len();
            info!(
                num_epochs,
                spo_count, "Tracking {num_epochs} epochs, latest snapshot has {spo_count} SPOs"
            );
        } else {
            info!("SPDD state: no data yet");
        }
        Ok(())
    }
}
