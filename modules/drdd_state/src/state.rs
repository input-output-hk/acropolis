use acropolis_common::DRepCredential;
use imbl::OrdMap;
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
        let latest = self.historical_distributions.iter().last();

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
