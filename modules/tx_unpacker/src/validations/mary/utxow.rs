//! Mary era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/mary/impl/src/Cardano/Ledger/Mary/Rules/Utxow.hs#L25
//!
//! NOTE: Mary UTxOW uses the same validation rules as Shelley UTxOW.

use crate::validations::shelley;
use acropolis_common::{
    validation::UTxOWValidationError, GenesisDelegates, NativeScript, TxHash, VKeyWitness,
};
use pallas::ledger::primitives::alonzo;

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

    Ok(())
}
