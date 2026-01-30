//! Alonzo era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L318
//!
//! NOTE: Alonzo UTxOW re-uses Shelley UTxOW rules, but introduces several new validation rules.

use std::collections::{HashMap, HashSet};

use crate::validations::shelley;
use acropolis_common::{
    crypto::keyhash_256, hash::Hash, validation::UTxOWValidationError, GenesisDelegates,
    NativeScript, ScriptIntegrityHash, TxHash, VKeyWitness,
};
use pallas::{
    codec::{
        minicbor::{encode, Encoder},
        utils::KeepRaw,
    },
    ledger::primitives::alonzo,
};

/// Script integrity hash input bytes:
/// redeemers ++ (if plutus data non-empty then plutus data else []) ++ lang_views.
fn compute_script_integrity_hash(
    redeemer: &[alonzo::Redeemer],
    plutus_data: &[alonzo::PlutusData],
    used_plutusv1_script: bool,
) -> Option<ScriptIntegrityHash> {
    if redeemer.is_empty() && plutus_data.is_empty() {
        return None;
    }
    let mut value_to_hash: Vec<u8> = Vec::new();

    // First, the Redeemer.
    let _ = encode(redeemer, &mut value_to_hash);

    // Next, the PlutusData (definite-length array encoding).
    let mut plutus_data_encoder: Encoder<Vec<u8>> = Encoder::new(Vec::new());
    let _ = plutus_data_encoder.array(plutus_data.len() as u64);
    for single_plutus_data in plutus_data.iter() {
        let _ = plutus_data_encoder.encode(single_plutus_data);
    }
    value_to_hash.extend(plutus_data_encoder.writer().clone());

    // Finally, the cost model.
    if used_plutusv1_script {
        value_to_hash.extend(plutus_language_views_cbor());
    } else {
        let empty_lang_views = HashMap::<Vec<u8>, Vec<u8>>::new();
        let _ = encode(empty_lang_views, &mut value_to_hash);
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

    let used_plutusv1_script =
        mtx.transaction_witness_set.plutus_script.as_ref().map(|x| !x.is_empty()).unwrap_or(false);

    let redeemers = if let Some(redeemers) = mtx.transaction_witness_set.redeemer.as_ref() {
        redeemers
    } else {
        &vec![]
    };
    let plutus_data = if let Some(plutus_data) = mtx.transaction_witness_set.plutus_data.as_ref() {
        plutus_data.iter().map(|x| KeepRaw::unwrap(x.clone())).collect::<Vec<alonzo::PlutusData>>()
    } else {
        vec![]
    };

    let computed_hash =
        compute_script_integrity_hash(redeemers, &plutus_data, used_plutusv1_script);

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
pub fn validate(
    mtx: &alonzo::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &HashSet<VKeyWitness>,
    native_scripts: &[NativeScript],
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
) -> Result<(), Box<UTxOWValidationError>> {
    shelley::utxow::validate(
        mtx,
        tx_hash,
        vkey_witnesses,
        native_scripts,
        genesis_delegs,
        update_quorum,
    )?;

    validate_script_integrity_hash(mtx)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::TestContext, validation_fixture};
    use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
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
    #[allow(clippy::result_large_err)]
    fn alonzo_test((ctx, raw_tx): (TestContext, Vec<u8>)) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(PallasEra::Alonzo, &raw_tx).unwrap();
        let mtx = tx.as_alonzo().unwrap();
        let vkey_witnesses = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses()).0;
        let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());
        validate(
            mtx,
            TxHash::from(*tx.hash()),
            &vkey_witnesses,
            &native_scripts,
            &ctx.shelley_params.gen_delegs,
            ctx.shelley_params.update_quorum,
        )
        .map_err(|e| *e)
    }
}
