//! Alonzo era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L318
//!
//! NOTE: Alonzo UTxOW re-uses Shelley UTxOW rules, but introduces several new validation rules.

use std::collections::HashSet;

use crate::validations::shelley;
use acropolis_common::{
    crypto::keyhash_256, hash::Hash, validation::UTxOWValidationError, GenesisDelegates,
    NativeScript, ScriptIntegrityHash, TxHash, VKeyWitness,
};
use pallas::{
    codec::{
        minicbor::{self, Encoder},
        utils::KeepRaw,
    },
    ledger::primitives::alonzo,
};

fn option_vec_is_empty<T>(option_vec: &Option<Vec<T>>) -> bool {
    option_vec.as_ref().map(|vec| vec.is_empty()).unwrap_or(true)
}

fn compute_script_integrity_hash(
    plutus_data: &[alonzo::PlutusData],
    redeemer: &[alonzo::Redeemer],
) -> ScriptIntegrityHash {
    let mut value_to_hash: Vec<u8> = Vec::new();
    // First, the Redeemer.
    let _ = minicbor::encode(redeemer, &mut value_to_hash);
    // Next, the PlutusData.
    let mut plutus_data_encoder: Encoder<Vec<u8>> = Encoder::new(Vec::new());
    let _ = plutus_data_encoder.begin_array();
    for single_plutus_data in plutus_data.iter() {
        let _ = plutus_data_encoder.encode(single_plutus_data);
    }
    let _ = plutus_data_encoder.end();
    value_to_hash.extend(plutus_data_encoder.writer().clone());
    // Finally, the cost model.
    value_to_hash.extend(cost_model_cbor());
    keyhash_256(&value_to_hash)
}

fn cost_model_cbor() -> Vec<u8> {
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

    let has_plutus_script =
        mtx.transaction_witness_set.plutus_script.as_ref().map(|x| !x.is_empty()).unwrap_or(false);

    if has_plutus_script {
        match script_data_hash {
            Some(script_data_hash) => {
                match (
                    &mtx.transaction_witness_set.plutus_data,
                    &mtx.transaction_witness_set.redeemer,
                ) {
                    (Some(plutus_data), Some(redeemer)) => {
                        let plutus_data = plutus_data
                            .iter()
                            .map(|x| KeepRaw::unwrap(x.clone()))
                            .collect::<Vec<alonzo::PlutusData>>();
                        let computed_hash = compute_script_integrity_hash(&plutus_data, redeemer);
                        if script_data_hash == computed_hash {
                            Ok(())
                        } else {
                            Err(Box::new(
                                UTxOWValidationError::ScriptIntegrityHashMismatch {
                                    expected: Some(computed_hash),
                                    actual: Some(script_data_hash),
                                    reason: "Script integrity hash mismatch".to_string(),
                                },
                            ))
                        }
                    }
                    _ => Err(Box::new(
                        UTxOWValidationError::ScriptIntegrityHashMismatch {
                            expected: None,
                            actual: None,
                            reason: "Missing plutus data or redeemer".to_string(),
                        },
                    )),
                }
            }
            None => {
                if option_vec_is_empty(&mtx.transaction_witness_set.plutus_data)
                    && option_vec_is_empty(&mtx.transaction_witness_set.redeemer)
                {
                    Ok(())
                } else {
                    Err(Box::new(
                        UTxOWValidationError::ScriptIntegrityHashMismatch {
                            expected: None,
                            actual: None,
                            reason: "Missing script data hash".to_string(),
                        },
                    ))
                }
            }
        }
    } else {
        match script_data_hash {
            Some(script_data_hash) => Err(Box::new(
                UTxOWValidationError::ScriptIntegrityHashMismatch {
                    expected: None,
                    actual: Some(script_data_hash),
                    reason: "Script data hash set without plutus script".to_string(),
                },
            )),
            None => Ok(()),
        }
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
