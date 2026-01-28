//! Babbage era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/Utxow.hs#L287
//!
//! NOTE: Babbage UTxOW re-uses Alonzo UTxOW rules, but introduces several new validation rules.

use std::collections::HashSet;

use crate::validations::shelley;
use acropolis_common::{
    validation::UTxOWValidationError, GenesisDelegates, NativeScript, TxHash, VKeyWitness,
};
use pallas::ledger::primitives::babbage;

/// NEW Babbage Validation Rules
/// Since Babbage introduces **reference scripts** and **inline datums**, this requires new UTxOW validation rules.
///
/// 1. MalformedScriptWitnesses
/// 2. MalformedReferenceScripts
pub fn validate(
    mtx: &babbage::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &[VKeyWitness],
    native_scripts: &[NativeScript],
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
) -> Result<(), Box<UTxOWValidationError>> {
    shelley_wrapper(
        mtx,
        tx_hash,
        vkey_witnesses,
        native_scripts,
        genesis_delegs,
        update_quorum,
    )?;

    // TODO:
    // Add ScriptIntegrityHash validation here

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

fn shelley_wrapper(
    mtx: &babbage::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &[VKeyWitness],
    native_scripts: &[NativeScript],
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
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
