//! Babbage era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/Utxow.hs#L287
//!
//! NOTE: Babbage UTxOW re-uses Alonzo UTxOW rules, but introduces several new validation rules.

use std::collections::HashSet;

use crate::validations::shelley;
use acropolis_common::{
    protocol_params::ProtocolVersion, validation::UTxOWValidationError, DataHash, GenesisDelegates,
    Metadata, NativeScript, ReferenceScript, TxHash, VKeyWitness,
};
use pallas::{codec::utils::Nullable, ledger::primitives::babbage};
use rayon::prelude::*;
use uplc_turbo::{arena::Arena, binder::DeBruijn, flat, program::Program};

fn get_aux_data_hash(
    mtx: &babbage::MintedTx,
) -> Result<Option<DataHash>, Box<UTxOWValidationError>> {
    let aux_data_hash = match mtx.transaction_body.auxiliary_data_hash.as_ref() {
        Some(x) => Some(DataHash::try_from(x.to_vec()).map_err(|_| {
            Box::new(UTxOWValidationError::InvalidMetadataHash {
                reason: "invalid metadata hash".to_string(),
            })
        })?),
        None => None,
    };
    Ok(aux_data_hash)
}

fn get_aux_data(mtx: &babbage::MintedTx) -> Option<Vec<u8>> {
    match &mtx.auxiliary_data {
        Nullable::Some(x) => Some(x.raw_cbor().to_vec()),
        _ => None,
    }
}

/// NEW Babbage Validation Rules
/// Since Babbage introduces **reference scripts** and **inline datums**, this requires new UTxOW validation rules.
///
/// 1. MalformedScriptWitnesses
/// 2. MalformedReferenceScripts
#[allow(clippy::too_many_arguments)]
pub fn validate(
    mtx: &babbage::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &[VKeyWitness],
    native_scripts: &[NativeScript],
    plutus_scripts_witnesses: &[ReferenceScript],
    metadata: &Option<Metadata>,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
    protocol_version: &ProtocolVersion,
) -> Result<(), Box<UTxOWValidationError>> {
    shelley_wrapper(
        mtx,
        tx_hash,
        vkey_witnesses,
        native_scripts,
        metadata,
        genesis_delegs,
        update_quorum,
        protocol_version,
    )?;

    // TODO:
    // Add ScriptIntegrityHash validation here

    validate_plutus_scripts_witnesses(plutus_scripts_witnesses)?;

    Ok(())
}

fn has_mir_certificate(mtx: &babbage::MintedTx) -> bool {
    mtx.transaction_body
        .certificates
        .as_ref()
        .map(|certs| {
            certs
                .iter()
                .any(|cert| matches!(cert, babbage::Certificate::MoveInstantaneousRewardsCert(_)))
        })
        .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
fn shelley_wrapper(
    mtx: &babbage::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &[VKeyWitness],
    native_scripts: &[NativeScript],
    metadata: &Option<Metadata>,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
    protocol_version: &ProtocolVersion,
) -> Result<(), Box<UTxOWValidationError>> {
    let transaction_body = &mtx.transaction_body;

    // Extract vkey hashes from vkey_witnesses
    let vkey_hashes_provided = vkey_witnesses.iter().map(|w| w.key_hash()).collect::<HashSet<_>>();

    // validate native scripts
    shelley::utxow::validate_native_scripts(
        native_scripts,
        &vkey_hashes_provided,
        transaction_body.validity_interval_start,
        transaction_body.ttl,
    )?;

    // validate vkey witnesses signatures
    shelley::utxow::validate_vkey_witnesses(vkey_witnesses, tx_hash)?;

    // validate metadata
    shelley::utxow::validate_metadata(
        get_aux_data_hash(mtx)?,
        get_aux_data(mtx),
        metadata,
        protocol_version,
    )?;

    // validate mir certificate genesis sig
    if has_mir_certificate(mtx) {
        shelley::utxow::validate_mir_genesis_sigs(
            &vkey_hashes_provided,
            genesis_delegs,
            update_quorum,
        )?;
    }

    Ok(())
}

/// Validate Plutus Script Witnesses are well formed
/// This is added from Babbage era.
/// Native scripts are not considered here. (Native script is valid one if that is decoded as NativeScript.)
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/Utxow.hs#L254
pub fn validate_plutus_scripts_witnesses(
    plutus_scripts_witnesses: &[ReferenceScript],
) -> Result<(), Box<UTxOWValidationError>> {
    plutus_scripts_witnesses.par_iter().try_for_each(validate_script_wellformedness)?;

    Ok(())
}

fn validate_script_wellformedness(
    reference_script: &ReferenceScript,
) -> Result<(), Box<UTxOWValidationError>> {
    let script_bytes = match reference_script {
        ReferenceScript::PlutusV1(bytes) => bytes,
        ReferenceScript::PlutusV2(bytes) => bytes,
        ReferenceScript::PlutusV3(bytes) => bytes,
        _ => return Ok(()),
    };

    let arena = Arena::new();
    let _: &Program<DeBruijn> = flat::decode(&arena, script_bytes).map_err(|e| {
        Box::new(UTxOWValidationError::MalformedScriptWitnesses {
            script_hash: reference_script.compute_hash(),
            reason: format!("Invalid script: {}", e),
        })
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use pallas::ledger::traverse::MultiEraTx;

    use crate::test_utils::{to_pallas_era, TestContext};
    use crate::validation_fixture;
    use test_case::test_case;

    use super::*;

    #[test]
    fn test_validate_script_wellformedness() {
        let script = ReferenceScript::PlutusV2(hex::decode("59014c01000032323232323232322223232325333009300e30070021323233533300b3370e9000180480109118011bae30100031225001232533300d3300e22533301300114a02a66601e66ebcc04800400c5288980118070009bac3010300c300c300c300c300c300c300c007149858dd48008b18060009baa300c300b3754601860166ea80184ccccc0288894ccc04000440084c8c94ccc038cd4ccc038c04cc030008488c008dd718098018912800919b8f0014891ce1317b152faac13426e6a83e06ff88a4d62cce3c1634ab0a5ec133090014a0266008444a00226600a446004602600a601a00626600a008601a006601e0026ea8c03cc038dd5180798071baa300f300b300e3754601e00244a0026eb0c03000c92616300a001375400660106ea8c024c020dd5000aab9d5744ae688c8c0088cc0080080048c0088cc00800800555cf2ba15573e6e1d200201").unwrap());
        let result = validate_script_wellformedness(&script);
        assert!(result.is_ok());
    }

    #[test_case(validation_fixture!(
        "babbage",
        "2f0468a9b39a46eecd5576bc440895fc968a6aefe504341ad5a59b5f60d299de"
    ) =>
        matches Ok(());
        "babbage - valid transaction 1 with 4 plutus scripts witnesses"
    )]
    #[allow(clippy::result_large_err)]
    fn babbage_utxow_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let mtx = tx.as_babbage().unwrap();
        let vkey_witnesses = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses()).0;
        let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());
        let metadata = acropolis_codec::map_metadata(&tx.metadata());
        let plutus_scripts_witnesses = acropolis_codec::extract_plutus_scripts_witnesses(&tx);

        validate(
            mtx,
            TxHash::from(*tx.hash()),
            &vkey_witnesses,
            &native_scripts,
            &plutus_scripts_witnesses,
            &metadata,
            &ctx.shelley_params.gen_delegs,
            ctx.shelley_params.update_quorum,
            &ctx.shelley_params.protocol_params.protocol_version,
        )
        .map_err(|e| *e)
    }
}
