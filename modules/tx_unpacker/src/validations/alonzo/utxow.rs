//! Alonzo era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L318
//!
//! NOTE: Alonzo UTxOW re-uses Shelley UTxOW rules, but introduces several new validation rules.

use std::collections::HashSet;

use crate::validations::shelley;
use acropolis_common::{
    protocol_params::ProtocolVersion, validation::UTxOWValidationError, GenesisDelegates, Metadata,
    NativeScript, TxHash, VKeyWitness,
};
use pallas::ledger::primitives::alonzo;

fn _cost_model_cbor() -> Vec<u8> {
    // Mainnet, preprod and preview all have the same cost model during the Alonzo
    // era.
    hex::decode(
        "a141005901d59f1a000302590001011a00060bc719026d00011a000249f01903e800011a000249f018201a0025cea81971f70419744d186419744d186419744d186419744d186419744d186419744d18641864186419744d18641a000249f018201a000249f018201a000249f018201a000249f01903e800011a000249f018201a000249f01903e800081a000242201a00067e2318760001011a000249f01903e800081a000249f01a0001b79818f7011a000249f0192710011a0002155e19052e011903e81a000249f01903e8011a000249f018201a000249f018201a000249f0182001011a000249f0011a000249f0041a000194af18f8011a000194af18f8011a0002377c190556011a0002bdea1901f1011a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a000242201a00067e23187600010119f04c192bd200011a000249f018201a000242201a00067e2318760001011a000242201a00067e2318760001011a0025cea81971f704001a000141bb041a000249f019138800011a000249f018201a000302590001011a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a000249f018201a00330da70101ff"
    ).unwrap()
}

/// Validate Script Integrity Hash
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L289
pub fn validate_script_integrity_hash(
    _mtx: &alonzo::MintedTx,
) -> Result<(), Box<UTxOWValidationError>> {
    // TODO:
    // Implement script integrity hash validation
    Ok(())
}

/// NEW Alonzo Validation Rules
/// Since Alonzo introduces **Plutus Scripts** (phase 2), this requires new UTxOW validation rules.
///
/// 1. ScriptIntegrityHashMismatch
#[allow(clippy::too_many_arguments)]
pub fn validate(
    mtx: &alonzo::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &HashSet<VKeyWitness>,
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
