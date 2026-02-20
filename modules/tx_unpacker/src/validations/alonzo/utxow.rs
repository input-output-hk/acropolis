//! Alonzo era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L318
//!
//! NOTE: Alonzo UTxOW re-uses Shelley UTxOW rules, but introduces several new validation rules.

use std::collections::HashMap;

use crate::validations::shelley;
use acropolis_common::{
    crypto::keyhash_256, hash::Hash, protocol_params::ProtocolVersion,
    validation::UTxOWValidationError, GenesisDelegates, Metadata, NativeScript,
    ScriptIntegrityHash, TxHash, VKeyWitness,
};
use anyhow::Result;
use pallas::{
    codec::{minicbor, utils::AnyCbor},
    ledger::primitives::alonzo,
};
use tracing::error;

/// Extract raw CBOR bytes for witness set map key 4 (plutus_data) and key 5 (redeemer)
/// so that the exact on-chain encoding is preserved (e.g. indefinite-length 9f...ff
/// is not normalized to definite-length 81...).
#[allow(clippy::type_complexity)]
fn extract_raw_witness_script_data(
    raw_witness_set: &[u8],
) -> Result<(Option<Vec<u8>>, Option<Vec<u8>>)> {
    let mut decoder = minicbor::Decoder::new(raw_witness_set);
    let mut plutus_data_raw: Option<Vec<u8>> = None;
    let mut redeemer_raw: Option<Vec<u8>> = None;
    let iter = decoder.map_iter::<u64, AnyCbor>()?;
    for pair in iter {
        let (key, value) = pair?;
        match key {
            4 => plutus_data_raw = Some(value.raw_bytes().to_vec()),
            5 => redeemer_raw = Some(value.raw_bytes().to_vec()),
            _ => {}
        }
    }
    Ok((plutus_data_raw, redeemer_raw))
}

/// Script integrity hash input bytes:
/// redeemers ++ (if plutus data non-empty then plutus data else []) ++ lang_views.
/// Uses the original CBOR bytes from the witness set so indefinite-length encodings
/// (e.g. 9f...ff) are preserved and the hash matches on-chain.
fn compute_script_integrity_hash(mtx: &alonzo::MintedTx) -> Option<ScriptIntegrityHash> {
    let raw_witness_set = mtx.transaction_witness_set.raw_cbor();
    let (plutus_data_raw, redeemer_raw) = match extract_raw_witness_script_data(raw_witness_set) {
        Ok(x) => x,
        Err(_) => {
            error!("Failed to extract raw witness script data");
            return None;
        }
    };

    let has_redeemer =
        mtx.transaction_witness_set.redeemer.as_ref().map(|r| !r.is_empty()).unwrap_or(false);
    let plutus_data_non_empty =
        mtx.transaction_witness_set.plutus_data.as_ref().map(|pd| !pd.is_empty()).unwrap_or(false);

    if !has_redeemer && !plutus_data_non_empty {
        return None;
    }

    let used_plutusv1_script =
        mtx.transaction_witness_set.plutus_script.as_ref().map(|x| !x.is_empty()).unwrap_or(false);

    let mut value_to_hash: Vec<u8> = Vec::new();

    // First, the Redeemer (original CBOR bytes, or empty array 0x80 when absent).
    match redeemer_raw {
        Some(r) => value_to_hash.extend(r),
        None => value_to_hash.push(0x80), // CBOR empty array
    }

    // Next, the PlutusData (original CBOR bytes) only when non-empty.
    if plutus_data_non_empty {
        if let Some(pd) = plutus_data_raw {
            value_to_hash.extend(pd);
        }
    }

    // Finally, the cost model.
    if used_plutusv1_script {
        value_to_hash.extend(plutus_language_views_cbor());
    } else {
        let empty_lang_views = HashMap::<Vec<u8>, Vec<u8>>::new();
        let _ = minicbor::encode(empty_lang_views, &mut value_to_hash);
    }

    Some(keyhash_256(&value_to_hash))
}

fn plutus_language_views_cbor() -> Vec<u8> {
    // Mainnet, preprod and preview all have the same cost model during the Alonzo
    // era.
    hex::decode(
        "a141005901d59f1a000302590001011a00060bc719026d00011a000249f01903e800011a000249f018201a0025cea81971f70419744d186419744d186419744d186419744d186419744d186419744d18641864186419744d18641a000249f018201a000249f018201a000249f018201a000249f01903e800011a000249f018201a000249f01903e800081a000242201a00067e2318760001011a000249f01903e800081a000249f01a0001b79818f7011a000249f0192710011a0002155e19052e011903e81a000249f01903e8011a000249f018201a000249f018201a000249f0182001011a000249f0011a000249f0041a000194af18f8011a000194af18f8011a0002377c190556011a0002bdea1901f1011a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a000242201a00067e23187600010119f04c192bd200011a000249f018201a000242201a00067e2318760001011a000242201a00067e2318760001011a0025cea81971f704001a000141bb041a000249f019138800011a000249f018201a000302590001011a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a00330da70101ff"
    ).unwrap()
}

/// Validate Script Integrity Hash
/// Reference: https://github.com/IntersectMBO/cardano-ledgeFr/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L289
pub fn validate_script_integrity_hash(
    mtx: &alonzo::MintedTx,
) -> Result<(), Box<UTxOWValidationError>> {
    let script_data_hash =
        mtx.transaction_body.script_data_hash.as_ref().map(|x| Hash::<32>::from(**x));
    let computed_hash = compute_script_integrity_hash(mtx);

    if script_data_hash.eq(&computed_hash) {
        Ok(())
    } else {
        Err(Box::new(
            UTxOWValidationError::ScriptIntegrityHashMismatch {
                expected: computed_hash,
                actual: script_data_hash,
                reason: "Script integrity hash mismatch".to_string(),
            },
        ))
    }
}

/// NEW Alonzo Validation Rules
/// Since Alonzo introduces **Plutus Scripts** (phase 2), this requires new UTxOW validation rules.
///
/// 1. ScriptIntegrityHashMismatch
#[allow(clippy::too_many_arguments)]
pub fn validate(
    mtx: &alonzo::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &[VKeyWitness],
    native_scripts: &[NativeScript],
    metadata: &Option<Metadata>,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
    protocol_version: &ProtocolVersion,
) -> Result<(), Box<UTxOWValidationError>> {
    shelley::utxow::validate(
        mtx,
        tx_hash,
        vkey_witnesses,
        native_scripts,
        metadata,
        genesis_delegs,
        update_quorum,
        protocol_version,
    )?;

    validate_script_integrity_hash(mtx)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_utils::{to_pallas_era, TestContext},
        validation_fixture,
    };
    use pallas::codec::minicbor;
    use pallas::ledger::traverse::MultiEraTx;
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "alonzo",
        "97779c4e21031457206c64c4f6adee02287178ba24242de475c68d7fbe1f12ba"
    ) =>
        matches Ok(());
        "alonzo - valid transaction 1 - mint assets using native script"
    )]
    #[test_case(validation_fixture!(
        "alonzo",
        "137f32a8c6e55a5b85472ba13e9908160623a18877e9d0fa4f7a8c393df0560e"
    ) =>
        matches Ok(());
        "alonzo - valid transaction 2 - has plutus data, no redeemer"
    )]
    #[test_case(validation_fixture!(
        "alonzo",
        "de5a43595e3257b9cccb90a396c455a0ed3895a7d859fb507b85363ee4638590"
    ) =>
        matches Ok(());
        "alonzo - valid transaction 3 - has plutus data, contract, redeemer"
    )]
    #[test_case(validation_fixture!(
        "alonzo",
        "567070233c5328d572a371ea481351df043e536846d763ea593b730048f60e4c"
    ) =>
        matches Ok(());
        "alonzo - valid transaction 4 - has contract, no plutus data, redeemer"
    )]
    #[test_case(validation_fixture!(
        "alonzo",
        "94a4e70902256267f37d1bb0cf95a0d6e05d7f8ae06f901ce4c9554267c7006c"
    ) =>
        matches Ok(());
        "alonzo - valid transaction 5 - has contract, plutus data, redeemer"
    )]
    #[allow(clippy::result_large_err)]
    fn alonzo_utxow_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let mtx = tx.as_alonzo().unwrap();
        let vkey_witnesses = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses()).0;
        let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());
        let metadata = acropolis_codec::map_metadata(&tx.metadata());

        validate(
            mtx,
            TxHash::from(*tx.hash()),
            &vkey_witnesses,
            &native_scripts,
            &metadata,
            &ctx.shelley_params.gen_delegs,
            ctx.shelley_params.update_quorum,
            &ctx.shelley_params.protocol_params.protocol_version,
        )
        .map_err(|e| *e)
    }

    #[test]
    fn compute_script_integrity_hash_returns_none_without_plutus_inputs() {
        let (_ctx, raw_tx, era): (TestContext, Vec<u8>, &str) = validation_fixture!(
            "alonzo",
            "97779c4e21031457206c64c4f6adee02287178ba24242de475c68d7fbe1f12ba"
        );
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let mtx = tx.as_alonzo().unwrap();

        let computed = compute_script_integrity_hash(mtx);

        assert!(computed.is_none(), "expected no script integrity hash");
    }

    #[test]
    fn validate_script_integrity_hash_reports_mismatch_when_body_hash_missing() {
        let (_ctx, raw_tx, _era): (TestContext, Vec<u8>, &str) = validation_fixture!(
            "alonzo",
            "de5a43595e3257b9cccb90a396c455a0ed3895a7d859fb507b85363ee4638590"
        );
        let tx: alonzo::Tx = minicbor::decode(&raw_tx).unwrap();
        let mut tx = tx;
        tx.transaction_body.script_data_hash = None;
        let mutated_tx = minicbor::to_vec(tx).unwrap();
        let mtx: alonzo::MintedTx = minicbor::decode(&mutated_tx).unwrap();

        let err = validate_script_integrity_hash(&mtx).unwrap_err();
        match *err {
            UTxOWValidationError::ScriptIntegrityHashMismatch { expected, actual, .. } => {
                assert!(expected.is_some(), "expected a computed hash");
                assert!(actual.is_none(), "expected missing body hash");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
