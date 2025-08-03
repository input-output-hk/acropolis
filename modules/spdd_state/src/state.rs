use acropolis_common::{DelegatedStake, KeyHash};
use std::collections::BTreeMap;
use tracing::info;

pub struct State {
    historical_distributions: BTreeMap<u64, BTreeMap<KeyHash, DelegatedStake>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            historical_distributions: BTreeMap::new(),
        }
    }

    pub fn insert_spdd(&mut self, epoch: u64, spdd: BTreeMap<KeyHash, DelegatedStake>) {
        self.historical_distributions.insert(epoch, spdd);
    }

    pub fn get_latest(&self) -> Option<BTreeMap<KeyHash, DelegatedStake>> {
        self.historical_distributions.last_key_value().map(|(_, map)| map.clone())
    }

    pub fn get_epoch(&self, epoch: u64) -> Option<BTreeMap<KeyHash, DelegatedStake>> {
        self.historical_distributions.get(&epoch).cloned()
    }

    pub async fn tick(&self) -> anyhow::Result<()> {
        let num_epochs = self.historical_distributions.len();
        let latest = self.historical_distributions.iter().last();

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
