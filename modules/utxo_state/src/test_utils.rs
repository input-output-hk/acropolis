use std::{collections::HashMap, str::FromStr};

use acropolis_common::{
    protocol_params::ShelleyParams, Address, Datum, Era, ScriptRef, TxHash, UTXOValue,
    UTxOIdentifier, Value,
};
use pallas::ledger::traverse::Era as PallasEra;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct UTxOValueJson {
    pub address: String,
    pub value: Value,
    pub datum: Option<Datum>,
    pub script_ref: Option<ScriptRef>,
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
                            script_ref: v.script_ref.clone(),
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
    ($era:literal, $hash:literal) => {
        (
            $crate::include_context!(concat!($era, "/", $hash, "/context.json")),
            $crate::include_cbor!(concat!($era, "/", $hash, "/tx.cbor")),
            $era,
        )
    };
    ($era:literal, $hash:literal, $variant:literal) => {
        (
            $crate::include_context!(concat!($era, "/", $hash, "/context.json")),
            $crate::include_cbor!(concat!($era, "/", $hash, "/", $variant, ".cbor")),
            $era,
        )
    };
}

pub fn to_era(era: &str) -> Era {
    match era {
        "byron" => Era::Byron,
        "shelley" => Era::Shelley,
        "allegra" => Era::Allegra,
        "mary" => Era::Mary,
        "alonzo" => Era::Alonzo,
        "babbage" => Era::Babbage,
        "conway" => Era::Conway,
        _ => panic!("Invalid era: {}", era),
    }
}

pub fn to_pallas_era(era: &str) -> PallasEra {
    match era {
        "byron" => PallasEra::Byron,
        "shelley" => PallasEra::Shelley,
        "allegra" => PallasEra::Allegra,
        "mary" => PallasEra::Mary,
        "alonzo" => PallasEra::Alonzo,
        "babbage" => PallasEra::Babbage,
        "conway" => PallasEra::Conway,
        _ => panic!("Invalid era: {}", era),
    }
}
