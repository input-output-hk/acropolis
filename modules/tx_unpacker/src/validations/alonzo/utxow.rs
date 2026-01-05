//! Alonzo era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L318
//!
//! NOTE: Alonzo UTxOW re-uses Shelley UTxOW rules, but introduces several new validation rules.

use crate::validations::shelley;
use acropolis_common::{
    validation::UTxOWValidationError, GenesisDelegates, NativeScript, TxHash, VKeyWitness,
};
use pallas::ledger::primitives::alonzo;

/// NEW Alonzo Validation Rules
/// Since Alonzo introduces **Plutus Scripts** (phase 2), this requires new UTxOW validation rules.
///
/// 1. MissingRedeemers
/// 2. MissingRequireDatums
/// 3. NotAllowedSupplementalDatums
/// 4. PPViewHashesDontMatch
/// 5. UnspendableUTxONoDatumHash
/// 6. ExtraRedeemers
/// 7. ScriptIntegrityHashMismatch
pub fn validate(
    mtx: &alonzo::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &[VKeyWitness],
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

    // TODO:
    // Add new validation rules here
    // Issue: https://github.com/input-output-hk/acropolis/issues/546

    Ok(())
}
