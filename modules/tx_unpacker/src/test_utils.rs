use acropolis_common::Slot;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TestContext {
    pub current_slot: Slot,
}

#[macro_export]
macro_rules! include_cbor {
    ($filepath:expr) => {
        hex::decode(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/transactions/",
            $filepath,
        )))
        .expect(concat!("invalid cbor file: ", $filepath))
    };
}

#[macro_export]
macro_rules! include_context {
    ($filepath:expr) => {
        serde_json::from_str::<$crate::test_utils::TestContext>(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/transactions/",
            $filepath,
        )))
        .expect(concat!("invalid context file: ", $filepath))
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
            $crate::include_context!(concat!($hash, "/", $variant, "/context.json")),
            $crate::include_cbor!(concat!($hash, "/", $variant, "/tx.cbor")),
        )
    };
}
