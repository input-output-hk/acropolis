use crate::calculations::{slot_to_epoch_with_shelley_params, slot_to_timestamp_with_params};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisValues {
    pub byron_timestamp: u64,
    pub shelley_epoch: u64,
    pub shelley_epoch_len: u64,
}

impl GenesisValues {
    pub fn mainnet() -> Self {
        Self {
            byron_timestamp: 1506203091,
            shelley_epoch: 208,
            shelley_epoch_len: 432000,
        }
    }

    pub fn slot_to_epoch(&self, slot: u64) -> (u64, u64) {
        slot_to_epoch_with_shelley_params(slot, self.shelley_epoch, self.shelley_epoch_len)
    }
    pub fn slot_to_timestamp(&self, slot: u64) -> u64 {
        slot_to_timestamp_with_params(slot, self.byron_timestamp, self.shelley_epoch)
    }
}
