use std::{collections::HashMap, str::FromStr};

use acropolis_common::{
    hash_script_ref, protocol_params::ShelleyParams, Address, Datum, ReferenceScript, TxHash,
    UTXOValue, UTxOIdentifier, Value,
};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct UTxOValueJson {
    pub address: String,
    pub value: Value,
    pub datum: Option<Datum>,
    pub reference_script: Option<ReferenceScript>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TestContextJson {
    pub shelley_params: ShelleyParams,
    pub utxos: HashMap<String, UTxOValueJson>,
}

#[derive(Debug)]
pub struct TestContext {
    pub shelley_params: ShelleyParams,
    pub utxos: HashMap<UTxOIdentifier, UTXOValue>,
}

impl From<TestContextJson> for TestContext {
    fn from(json: TestContextJson) -> Self {
        Self {
            shelley_params: json.shelley_params,
            utxos: json
                .utxos
                .iter()
                .map(|(k, v)| {
                    let tx_hash = TxHash::from_str(k.split('#').nth(0).unwrap()).unwrap();
                    let tx_index = k.split('#').nth(1).unwrap().parse::<u16>().unwrap();
                    (
                        UTxOIdentifier::new(tx_hash, tx_index),
                        UTXOValue {
                            address: Address::from_string(&v.address).unwrap(),
                            value: v.value.clone(),
                            datum: v.datum.clone(),
                            reference_script_hash: hash_script_ref(v.reference_script.clone()),
                        },
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
