use acropolis_common::{DelegatedStake, Epoch, PoolId};
use imbl::{HashMap, OrdMap, OrdSet};
use tracing::info;

#[derive(Clone, Default)]
pub struct State {
    spdd_history: OrdMap<Epoch, HashMap<PoolId, DelegatedStake>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            spdd_history: OrdMap::new(),
        }
    }

    pub fn apply_spdd_snapshot<I>(&mut self, epoch: Epoch, snapshot: I)
    where
        I: IntoIterator<Item = (PoolId, DelegatedStake)>,
    {
        let mut next = if epoch == 0 {
            Default::default()
        } else {
            self.spdd_history.get(&(epoch - 1)).cloned().unwrap_or_default()
        };

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

        self.spdd_history.insert(epoch, next);
    }

    #[allow(dead_code)]
    pub fn get_latest(&self) -> Option<&HashMap<PoolId, DelegatedStake>> {
        self.spdd_history.iter().next_back().map(|(_, v)| v)
    }

    #[allow(dead_code)]
    pub fn get_epoch(&self, epoch: Epoch) -> Option<&HashMap<PoolId, DelegatedStake>> {
        self.spdd_history.get(&epoch)
    }

    // Since this is active stakes
    // we plus 2 to epoch number
    pub fn get_epoch_total_active_stakes(&self, epoch: Epoch) -> Option<u64> {
        if epoch <= 2 {
            None
        } else {
            self.spdd_history.get(&(epoch - 2)).map(|state| state.values().map(|v| v.active).sum())
        }
    }

    pub async fn tick(&self) -> anyhow::Result<()> {
        if let Some((_, latest)) = self.spdd_history.iter().next_back() {
            let spo_count = latest.len();
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
