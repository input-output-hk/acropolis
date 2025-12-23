use acropolis_common::{protocol_params::ShelleyParams, Slot};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TestContextJson {
    pub shelley_params: ShelleyParams,
    pub current_slot: Slot,
}

#[derive(Debug)]
pub struct TestContext {
    pub shelley_params: ShelleyParams,
    pub current_slot: Slot,
}

impl From<TestContextJson> for TestContext {
    fn from(json: TestContextJson) -> Self {
        Self {
            shelley_params: json.shelley_params,
            current_slot: json.current_slot,
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
        )
    };
    ($era:literal, $hash:literal, $variant:literal) => {
        (
            $crate::include_context!(concat!($era, "/", $hash, "/context.json")),
            $crate::include_cbor!(concat!($era, "/", $hash, "/", $variant, ".cbor")),
        )
    };
}
