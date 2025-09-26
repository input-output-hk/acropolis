use crate::calculations::{
    epoch_to_first_slot_with_shelley_params, slot_to_epoch_with_shelley_params,
    slot_to_timestamp_with_params,
};
const MAINNET_SHELLEY_GENESIS_HASH: &str =
    "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisValues {
    pub byron_timestamp: u64,
    pub shelley_epoch: u64,
    pub shelley_epoch_len: u64,
    pub shelley_genesis_hash: [u8; 32],
}

impl GenesisValues {
    pub fn mainnet() -> Self {
        Self {
            byron_timestamp: 1506203091,
            shelley_epoch: 208,
            shelley_epoch_len: 432000,
            shelley_genesis_hash: hex::decode(MAINNET_SHELLEY_GENESIS_HASH)
                .unwrap()
                .try_into()
                .unwrap(),
        }
    }

    pub fn slot_to_epoch(&self, slot: u64) -> (u64, u64) {
        slot_to_epoch_with_shelley_params(slot, self.shelley_epoch, self.shelley_epoch_len)
    }
    pub fn slot_to_timestamp(&self, slot: u64) -> u64 {
        slot_to_timestamp_with_params(slot, self.byron_timestamp, self.shelley_epoch)
    }

    pub fn epoch_to_first_slot(&self, epoch: u64) -> u64 {
        epoch_to_first_slot_with_shelley_params(epoch, self.shelley_epoch, self.shelley_epoch_len)
    }
}
