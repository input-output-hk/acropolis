use crate::{
    calculations::{
        epoch_to_first_slot_with_shelley_params, slot_to_epoch_with_shelley_params,
        slot_to_timestamp_with_params,
    },
    hash::Hash,
    GenesisDelegates, MagicNumber, NetworkId, Pots,
};
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct GenesisValues {
    pub byron_timestamp: u64,
    pub shelley_epoch: u64,
    pub shelley_epoch_len: u64,
    pub shelley_genesis_hash: Hash<32>,
    pub genesis_delegs: GenesisDelegates,
    pub magic_number: MagicNumber,
    pub security_param: u64,
    pub initial_pots: Pots,
}

impl GenesisValues {
    pub fn network_id(&self) -> NetworkId {
        match self.magic_number {
            MagicNumber(764824073) => NetworkId::Mainnet,
            _ => NetworkId::Testnet,
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
