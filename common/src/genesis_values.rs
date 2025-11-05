use crate::{
    calculations::{
        epoch_to_first_slot_with_shelley_params, slot_to_epoch_with_shelley_params,
        slot_to_timestamp_with_params,
    },
    GenesisDelegates,
};
const MAINNET_SHELLEY_GENESIS_HASH: &str =
    "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisValues {
    pub byron_timestamp: u64,
    pub shelley_epoch: u64,
    pub shelley_epoch_len: u64,
    pub shelley_genesis_hash: [u8; 32],
    pub genesis_delegs: GenesisDelegates,
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
            genesis_delegs: GenesisDelegates::try_from(vec![
                (
                    "ad5463153dc3d24b9ff133e46136028bdc1edbb897f5a7cf1b37950c",
                    (
                        "d9e5c76ad5ee778960804094a389f0b546b5c2b140a62f8ec43ea54d",
                        "64fa87e8b29a5b7bfbd6795677e3e878c505bc4a3649485d366b50abadec92d7",
                    ),
                ),
                (
                    "b9547b8a57656539a8d9bc42c008e38d9c8bd9c8adbb1e73ad529497",
                    (
                        "855d6fc1e54274e331e34478eeac8d060b0b90c1f9e8a2b01167c048",
                        "66d5167a1f426bd1adcc8bbf4b88c280d38c148d135cb41e3f5a39f948ad7fcc",
                    ),
                ),
                (
                    "60baee25cbc90047e83fd01e1e57dc0b06d3d0cb150d0ab40bbfead1",
                    (
                        "7f72a1826ae3b279782ab2bc582d0d2958de65bd86b2c4f82d8ba956",
                        "c0546d9aa5740afd569d3c2d9c412595cd60822bb6d9a4e8ce6c43d12bd0f674",
                    ),
                ),
                (
                    "f7b341c14cd58fca4195a9b278cce1ef402dc0e06deb77e543cd1757",
                    (
                        "69ae12f9e45c0c9122356c8e624b1fbbed6c22a2e3b4358cf0cb5011",
                        "6394a632af51a32768a6f12dac3485d9c0712d0b54e3f389f355385762a478f2",
                    ),
                ),
                (
                    "162f94554ac8c225383a2248c245659eda870eaa82d0ef25fc7dcd82",
                    (
                        "4485708022839a7b9b8b639a939c85ec0ed6999b5b6dc651b03c43f6",
                        "aba81e764b71006c515986bf7b37a72fbb5554f78e6775f08e384dbd572a4b32",
                    ),
                ),
                (
                    "2075a095b3c844a29c24317a94a643ab8e22d54a3a3a72a420260af6",
                    (
                        "6535db26347283990a252313a7903a45e3526ec25ddba381c071b25b",
                        "fcaca997b8105bd860876348fc2c6e68b13607f9bbd23515cd2193b555d267af",
                    ),
                ),
                (
                    "268cfc0b89e910ead22e0ade91493d8212f53f3e2164b2e4bef0819b",
                    (
                        "1d4f2e1fda43070d71bb22a5522f86943c7c18aeb4fa47a362c27e23",
                        "63ef48bc5355f3e7973100c371d6a095251c80ceb40559f4750aa7014a6fb6db",
                    ),
                ),
            ])
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
