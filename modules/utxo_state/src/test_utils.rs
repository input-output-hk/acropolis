use std::{collections::HashMap, str::FromStr};

use acropolis_common::{
    protocol_params::{
        AlonzoParams, BabbageParams, ByronParams, ConwayParams, ProtocolParams, ShelleyParams,
    },
    Address, AssetName, Datum, DatumHash, Era, NativeAsset, PlutusVersion, PolicyId,
    ReferenceScript, ScriptHash, ScriptLang, ScriptRef, TxHash, UTXOValue, UTxOIdentifier, Value,
};
use pallas::ledger::traverse::Era as PallasEra;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct UTxOValueJson {
    pub address: String,
    #[serde(deserialize_with = "deserialize_value")]
    pub value: Value,
    #[serde(default, deserialize_with = "deserialize_datum")]
    pub datum: Option<Datum>,
    #[serde(default, deserialize_with = "deserialize_script_ref")]
    pub script_ref: Option<ScriptRef>,
}

fn deserialize_script_ref<'de, D>(deserializer: D) -> Result<Option<ScriptRef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(serde::Deserialize)]
    struct RawScriptRef {
        script_hash: ScriptHash,
        script_lang: String,
    }

    let Some(raw) = Option::<RawScriptRef>::deserialize(deserializer)? else {
        return Ok(None);
    };

    let script_lang = match raw.script_lang.as_str() {
        "Native" => ScriptLang::Native,
        "PlutusV1" => ScriptLang::Plutus(PlutusVersion::V1),
        "PlutusV2" => ScriptLang::Plutus(PlutusVersion::V2),
        "PlutusV3" => ScriptLang::Plutus(PlutusVersion::V3),
        other => panic!("unknown script_lang: {other}"),
    };

    Ok(Some(ScriptRef {
        script_hash: raw.script_hash,
        script_lang,
    }))
}

fn deserialize_value<'de, D>(deserializer: D) -> Result<Value, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(serde::Deserialize)]
    struct RawValue {
        lovelace: u64,
        assets: serde_json::Value,
    }

    let raw = RawValue::deserialize(deserializer)?;

    let assets = match &raw.assets {
        // Blockfrost format: [{"unit": "<policy_id><asset_name>", "quantity": "N"}, ...]
        serde_json::Value::Array(entries)
            if entries.first().is_some_and(|e| e.get("unit").is_some()) =>
        {
            let mut map: HashMap<PolicyId, Vec<NativeAsset>> = HashMap::new();
            for entry in entries {
                let unit = entry["unit"].as_str().unwrap();
                let quantity: u64 = entry["quantity"].as_str().unwrap().parse().unwrap();
                let policy_id = PolicyId::from_str(&unit[..56]).unwrap();
                let asset_name = AssetName::new(&hex::decode(&unit[56..]).unwrap()).unwrap();
                map.entry(policy_id).or_default().push(NativeAsset {
                    name: asset_name,
                    amount: quantity,
                });
            }
            map.into_iter().collect()
        }
        // Native format: [["policy_id", [{"name": "...", "amount": N}]], ...]
        _ => serde_json::from_value(raw.assets).unwrap(),
    };

    Ok(Value::new(raw.lovelace, assets))
}

fn deserialize_datum<'de, D>(deserializer: D) -> Result<Option<Datum>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    let Some(raw) = Option::<serde_json::Value>::deserialize(deserializer)? else {
        return Ok(None);
    };

    let map = raw.as_object().unwrap();
    let datum = if let Some(val) = map.get("Inline") {
        Datum::Inline(hex::decode(val.as_str().unwrap()).unwrap())
    } else if let Some(val) = map.get("Hash") {
        Datum::Hash(serde_json::from_value::<DatumHash>(val.clone()).unwrap())
    } else {
        panic!("expected 'Inline' or 'Hash' datum variant");
    };

    Ok(Some(datum))
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TestContextJson {
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_protocol_params_with_defaults")]
    pub protocol_params: ProtocolParams,
    pub utxos: HashMap<String, UTxOValueJson>,
    #[serde(default, deserialize_with = "deserialize_reference_scripts")]
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

fn parse_reference_script(raw: &serde_json::Value) -> ReferenceScript {
    let obj = raw.as_object().unwrap();
    assert_eq!(obj.len(), 1, "expected exactly one variant key");
    let (variant, val) = obj.iter().next().unwrap();

    match variant.as_str() {
        "PlutusV1" => ReferenceScript::PlutusV1(hex::decode(val.as_str().unwrap()).unwrap()),
        "PlutusV2" => ReferenceScript::PlutusV2(hex::decode(val.as_str().unwrap()).unwrap()),
        "PlutusV3" => ReferenceScript::PlutusV3(hex::decode(val.as_str().unwrap()).unwrap()),
        "Native" => ReferenceScript::Native(serde_json::from_value(val.clone()).unwrap()),
        _ => panic!("unknown ReferenceScript variant: {variant}"),
    }
}

fn deserialize_reference_scripts<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, ReferenceScript>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    let map: HashMap<String, serde_json::Value> = HashMap::deserialize(deserializer)?;
    Ok(map.into_iter().map(|(k, v)| (k, parse_reference_script(&v))).collect())
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
