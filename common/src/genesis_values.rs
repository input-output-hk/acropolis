use std::str::FromStr;

use anyhow::{bail, Result};

use crate::{
    calculations::{
        epoch_to_first_slot_with_shelley_params, slot_to_epoch_with_shelley_params,
        slot_to_timestamp_with_params,
    },
    hash::Hash,
    GenesisDelegates, MagicNumber,
};

const MAINNET_SHELLEY_GENESIS_HASH: &str =
    "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81";
const PREVIEW_SHELLEY_GENESIS_HASH: &str =
    "bf772f645f1ef1701cb75eb1eb277fc63db0a130335187cea6b8a8148b3a0aaf";
const SANCHONET_SHELLEY_GENESIS_HASH: &str =
    "5023cadbedb36234bbc38cf03d0136aae092d9bada473c233419d45a0a6ed3b1";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct GenesisValues {
    pub byron_timestamp: u64,
    pub shelley_epoch: u64,
    pub shelley_epoch_len: u64,
    pub shelley_genesis_hash: Hash<32>,
    pub genesis_delegs: GenesisDelegates,
    pub magic_number: MagicNumber,
}

impl GenesisValues {
    pub fn mainnet() -> Self {
        Self {
            byron_timestamp: 1506203091,
            shelley_epoch: 208,
            shelley_epoch_len: 432000,
            shelley_genesis_hash: Hash::<32>::from_str(MAINNET_SHELLEY_GENESIS_HASH).unwrap(),
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
            magic_number: MagicNumber::new(764824073),
        }
    }

    pub fn preview() -> Self {
        Self {
            byron_timestamp: 1666656000,
            shelley_epoch: 0,
            shelley_epoch_len: 86400,
            shelley_genesis_hash: Hash::<32>::from_str(PREVIEW_SHELLEY_GENESIS_HASH).unwrap(),
            genesis_delegs: GenesisDelegates::try_from(vec![
                (
                    "12b0f443d02861948a0fce9541916b014e8402984c7b83ad70a834ce",
                    (
                        "7c54a168c731f2f44ced620f3cca7c2bd90731cab223d5167aa994e6",
                        "62d546a35e1be66a2b06e29558ef33f4222f1c466adbb59b52d800964d4e60ec",
                    ),
                ),
                (
                    "3df542796a64e399b60c74acfbdb5afa1e114532fa36b46d6368ef3a",
                    (
                        "c44bc2f3cc7e98c0f227aa399e4035c33c0d775a0985875fff488e20",
                        "4f9d334decadff6eba258b2df8ae1f02580a2628bce47ae7d957e1acd3f42a3c",
                    ),
                ),
                (
                    "93fd5083ff20e7ab5570948831730073143bea5a5d5539852ed45889",
                    (
                        "82a02922f10105566b70366b07c758c8134fa91b3d8ae697dfa5e8e0",
                        "8a57e94a9b4c65ec575f35d41edb1df399fa30fdf10775389f5d1ef670ca3f9f",
                    ),
                ),
                (
                    "a86cab3ea72eabb2e8aafbbf4abbd2ba5bdfd04eea26a39b126a78e4",
                    (
                        "10257f6d3bae913514bdc96c9170b3166bf6838cca95736b0e418426",
                        "1b54aad6b013145a0fc74bb5c2aa368ebaf3999e88637d78e09706d0cc29874a",
                    ),
                ),
                (
                    "b799804a28885bd49c0e1b99d8b3b26de0fac17a5cf651ecf0c872f0",
                    (
                        "ebe606e22d932d51be2c1ce87e7d7e4c9a7d1f7df4a5535c29e23d22",
                        "b3fc06a1f8ee69ff23185d9af453503be8b15b2652e1f9fb7c3ded6797a2d6f9",
                    ),
                ),
                (
                    "d125812d6ab973a2c152a0525b7fd32d36ff13555a427966a9cac9b1",
                    (
                        "e302198135fb5b00bfe0b9b5623426f7cf03179ab7ba75f945d5b79b",
                        "b45ca2ed95f92248fa0322ce1fc9f815a5a5aa2f21f1adc2c42c4dccfc7ba631",
                    ),
                ),
                (
                    "ef27651990a26449a40767d5e06cdef1670a3f3ff4b951d385b51787",
                    (
                        "0e0b11e80d958732e587585d30978d683a061831d1b753878f549d05",
                        "b860ec844f6cd476c4fabb4aa1ca72d5c74d82f3835aed3c9515a35b6e048719",
                    ),
                ),
            ])
            .unwrap(),
            magic_number: MagicNumber::new(2),
        }
    }

    pub fn sanchonet() -> Self {
        Self {
            byron_timestamp: 1686789000,
            shelley_epoch: 0,
            shelley_epoch_len: 86400,
            shelley_genesis_hash: Hash::<32>::from_str(SANCHONET_SHELLEY_GENESIS_HASH).unwrap(),
            genesis_delegs: GenesisDelegates::try_from(vec![
                (
                    "c1ad22cabb342cbb83ce3859708232f4945ccb669e9b5f932cffc0ed",
                    (
                        "405357b552c397e81f73dcb5a0da0828fe29610bd25197d86130df34",
                        "458215df6c07abc66e80082caa7a189dc2f4995ad4b4b5f09481a55d8d0692d2",
                    ),
                ),
                (
                    "c264bca994a3a5deee5a1d9b92a3d7e9d6cbdb81f2f6989bb7f7b437",
                    (
                        "d9d9d0f0e1f25c4af4d80cb2d62878b611d8b3a8e1ef548d01f246d7",
                        "624f1bf3b2f978e0c95644f26228b307d7acca7fc7eb3d88fb6f107e0aa1198c",
                    ),
                ),
                (
                    "d4bf7eb45b72dffa5ac33d5c902fe409e4e611f2e9a52fb0d09784c3",
                    (
                        "806eb0c17d9b0fe6d99acbabe7be76ef72bf9de96c5b58435e50837f",
                        "57e52289207a7128c29e0b7e96a02c731a961a5944329b363bed751ad8f377ee",
                    ),
                ),
            ])
            .unwrap(),
            magic_number: MagicNumber::new(4),
        }
    }

    pub fn for_network(network_name: &str) -> Result<Self> {
        match network_name {
            "mainnet" => Ok(Self::mainnet()),
            "preview" => Ok(Self::preview()),
            "sanchonet" | "sancho" => Ok(Self::sanchonet()),
            unsupported => bail!("Unsupported network for genesis values: {unsupported}"),
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
