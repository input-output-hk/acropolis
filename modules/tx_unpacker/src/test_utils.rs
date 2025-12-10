use acropolis_common::{
    protocol_params::ShelleyParams, Slot, TxHash, TxIdentifier, UTxOIdentifier,
};
use std::{collections::HashMap, str::FromStr};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TestContextJson {
    pub shelley_params: ShelleyParams,
    pub current_slot: Slot,
    // Vec<((TxHash, TxIndex), (BlockNumber, TxIndex))>
    pub utxos: Vec<((String, u16), (u32, u16))>,
}

#[derive(Debug)]
pub struct TestContext {
    pub shelley_params: ShelleyParams,
    pub current_slot: Slot,
    pub utxos: HashMap<UTxOIdentifier, TxIdentifier>,
}

impl From<TestContextJson> for TestContext {
    fn from(json: TestContextJson) -> Self {
        Self {
            shelley_params: json.shelley_params,
            current_slot: json.current_slot,
            utxos: json
                .utxos
                .into_iter()
                .map(|((tx_hash, output_index), (block_number, tx_index))| {
                    (
                        UTxOIdentifier::new(TxHash::from_str(&tx_hash).unwrap(), output_index),
                        TxIdentifier::new(block_number, tx_index),
                    )
                })
                .collect(),
        }
    }
}
#[macro_export]
macro_rules! include_cbor {
    ($filepath:expr) => {
        hex::decode(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/",
            $filepath,
        )))
        .expect(concat!("invalid cbor file: ", $filepath))
    };
}

#[macro_export]
macro_rules! include_context {
    ($filepath:expr) => {
        serde_json::from_str::<$crate::test_utils::TestContextJson>(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/",
            $filepath,
        )))
        .expect(concat!("invalid context file: ", $filepath))
        .into()
    };
}

#[macro_export]
macro_rules! validation_fixture {
    ($hash:literal) => {
        (
            $crate::include_context!(concat!($hash, "/context.json")),
            $crate::include_cbor!(concat!($hash, "/tx.cbor")),
        )
    };
    ($hash:literal, $variant:literal) => {
        (
            $crate::include_context!(concat!($hash, "/", "/context.json")),
            $crate::include_cbor!(concat!($hash, "/", $variant, ".cbor")),
        )
    };
}
