use std::{collections::HashMap, str::FromStr};

use acropolis_common::{
    protocol_params::{
        AlonzoParams, BabbageParams, ByronParams, ConwayParams, ProtocolParams, ShelleyParams,
    },
    Address, Datum, Era, ReferenceScript, ScriptHash, ScriptRef, TxHash, UTXOValue, UTxOIdentifier,
    Value,
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
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_protocol_params_with_defaults")]
    pub protocol_params: ProtocolParams,
    pub utxos: HashMap<String, UTxOValueJson>,
    #[serde(default)]
    pub reference_scripts: HashMap<String, ReferenceScript>,
}

fn deep_merge(base: &mut serde_json::Value, overlay: serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (k, v) in overlay_map {
                deep_merge(base_map.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn deserialize_protocol_params_with_defaults<'de, D>(
    deserializer: D,
) -> Result<ProtocolParams, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    let overlay = serde_json::Value::deserialize(deserializer)?;

    let mut base_params = ProtocolParams::default();
    if let Some(obj) = overlay.as_object() {
        if obj.contains_key("shelley") {
            base_params.shelley = Some(ShelleyParams::default());
        }
        if obj.contains_key("alonzo") {
            base_params.alonzo = Some(AlonzoParams::default());
        }
        if obj.contains_key("byron") {
            base_params.byron = Some(ByronParams::default());
        }
        if obj.contains_key("babbage") {
            base_params.babbage = Some(BabbageParams::default());
        }
        if obj.contains_key("conway") {
            base_params.conway = Some(ConwayParams::default());
        }
    }

    let mut base = serde_json::to_value(base_params).map_err(serde::de::Error::custom)?;
    deep_merge(&mut base, overlay);
    serde_json::from_value(base).map_err(serde::de::Error::custom)
}

#[derive(Debug)]
pub struct TestContext {
    pub protocol_params: ProtocolParams,
    pub utxos: HashMap<UTxOIdentifier, UTXOValue>,
    #[allow(dead_code)]
    pub reference_scripts: HashMap<ScriptHash, ReferenceScript>,
}

impl From<TestContextJson> for TestContext {
    fn from(json: TestContextJson) -> Self {
        Self {
            protocol_params: json.protocol_params,
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
            reference_scripts: json
                .reference_scripts
                .iter()
                .map(|(k, v)| (ScriptHash::from_str(k).unwrap(), v.clone()))
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
