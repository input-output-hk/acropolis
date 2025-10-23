use std::collections::BTreeMap;

use crate::{
    calculations::{
        epoch_to_first_slot_with_shelley_params, slot_to_epoch_with_shelley_params,
        slot_to_timestamp_with_params,
    },
    hash::Hash,
};
const MAINNET_SHELLEY_GENESIS_HASH: &str =
    "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81";

pub type GenesisKey = Hash<28>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct GenDeleg {
    // Pool Id
    pub delegate: Hash<28>,
    pub vrf: Hash<32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisDelegs(pub BTreeMap<GenesisKey, GenDeleg>);

impl AsRef<BTreeMap<GenesisKey, GenDeleg>> for GenesisDelegs {
    fn as_ref(&self) -> &BTreeMap<GenesisKey, GenDeleg> {
        &self.0
    }
}

impl From<Vec<(String, (String, String))>> for GenesisDelegs {
    fn from(entries: Vec<(String, (String, String))>) -> Self {
        let map = entries
            .into_iter()
            .map(|(key_hash, (delegate, vrf))| {
                let key = Hash::new(
                    hex::decode(key_hash)
                        .expect("Invalid key hash hex string")
                        .try_into()
                        .expect("Invalid Genesis Key length"),
                );
                let delegate_hash = Hash::new(
                    hex::decode(delegate)
                        .expect("Invalid delegate hex string")
                        .try_into()
                        .expect("Invalid delegate hash length"),
                );
                let vrf_hash = Hash::new(
                    hex::decode(vrf)
                        .expect("Invalid VRF hex string")
                        .try_into()
                        .expect("Invalid VRF hash length"),
                );
                (
                    key,
                    GenDeleg {
                        delegate: delegate_hash,
                        vrf: vrf_hash,
                    },
                )
            })
            .collect();
        GenesisDelegs(map)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisValues {
    pub byron_timestamp: u64,
    pub shelley_epoch: u64,
    pub shelley_epoch_len: u64,
    pub shelley_genesis_hash: [u8; 32],
    pub genesis_delegs: GenesisDelegs,
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
            genesis_delegs: GenesisDelegs::from(vec![
                (
                    "ad5463153dc3d24b9ff133e46136028bdc1edbb897f5a7cf1b37950c".to_string(),
                    (
                        "d9e5c76ad5ee778960804094a389f0b546b5c2b140a62f8ec43ea54d".to_string(),
                        "64fa87e8b29a5b7bfbd6795677e3e878c505bc4a3649485d366b50abadec92d7"
                            .to_string(),
                    ),
                ),
                (
                    "b9547b8a57656539a8d9bc42c008e38d9c8bd9c8adbb1e73ad529497".to_string(),
                    (
                        "855d6fc1e54274e331e34478eeac8d060b0b90c1f9e8a2b01167c048".to_string(),
                        "66d5167a1f426bd1adcc8bbf4b88c280d38c148d135cb41e3f5a39f948ad7fcc"
                            .to_string(),
                    ),
                ),
                (
                    "60baee25cbc90047e83fd01e1e57dc0b06d3d0cb150d0ab40bbfead1".to_string(),
                    (
                        "7f72a1826ae3b279782ab2bc582d0d2958de65bd86b2c4f82d8ba956".to_string(),
                        "c0546d9aa5740afd569d3c2d9c412595cd60822bb6d9a4e8ce6c43d12bd0f674"
                            .to_string(),
                    ),
                ),
                (
                    "f7b341c14cd58fca4195a9b278cce1ef402dc0e06deb77e543cd1757".to_string(),
                    (
                        "69ae12f9e45c0c9122356c8e624b1fbbed6c22a2e3b4358cf0cb5011".to_string(),
                        "6394a632af51a32768a6f12dac3485d9c0712d0b54e3f389f355385762a478f2"
                            .to_string(),
                    ),
                ),
                (
                    "162f94554ac8c225383a2248c245659eda870eaa82d0ef25fc7dcd82".to_string(),
                    (
                        "4485708022839a7b9b8b639a939c85ec0ed6999b5b6dc651b03c43f6".to_string(),
                        "aba81e764b71006c515986bf7b37a72fbb5554f78e6775f08e384dbd572a4b32"
                            .to_string(),
                    ),
                ),
                (
                    "2075a095b3c844a29c24317a94a643ab8e22d54a3a3a72a420260af6".to_string(),
                    (
                        "6535db26347283990a252313a7903a45e3526ec25ddba381c071b25b".to_string(),
                        "fcaca997b8105bd860876348fc2c6e68b13607f9bbd23515cd2193b555d267af"
                            .to_string(),
                    ),
                ),
                (
                    "268cfc0b89e910ead22e0ade91493d8212f53f3e2164b2e4bef0819b".to_string(),
                    (
                        "1d4f2e1fda43070d71bb22a5522f86943c7c18aeb4fa47a362c27e23".to_string(),
                        "63ef48bc5355f3e7973100c371d6a095251c80ceb40559f4750aa7014a6fb6db"
                            .to_string(),
                    ),
                ),
            ]),
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
