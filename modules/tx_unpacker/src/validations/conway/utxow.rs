//! Conway era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxow.hs#L75
//!
//! NOTE: Conway UTxOW re-uses Babbage, Alonzo and Shelley UTxOW rules. Only one of Shelley UTxOW rules is removed.

use std::collections::HashSet;

use crate::validations::shelley;
use acropolis_common::{validation::UTxOWValidationError, NativeScript, TxHash, VKeyWitness};
use pallas::ledger::primitives::conway;

/// MIRInsufficientGenesisSigsUTXOW from Shelley UTxOW rules
/// is removed in Conway Era (no MIR in conway)
pub fn validate(
    mtx: &conway::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &[VKeyWitness],
    native_scripts: &[NativeScript],
) -> Result<(), Box<UTxOWValidationError>> {
    shelley_wrapper(mtx, tx_hash, vkey_witnesses, native_scripts)?;

    // TODO:
    // Add Babbage UTxOW transition here
    // Add Alonzo UTxOW transition here

    Ok(())
}

fn shelley_wrapper(
    mtx: &conway::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &[VKeyWitness],
    native_scripts: &[NativeScript],
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

    // TODO:
    // Validate metadata
    // issue: https://github.com/input-output-hk/acropolis/issues/489

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{test_utils::TestContext, validation_fixture};
    use acropolis_common::KeyHash;
    use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "conway",
        "1b3a99a110ef5cc8d64f6a3d6ac0a8b3467104f1c29d306eb0293d563e962034"
    ) =>
        matches Ok(());
        "valid transaction 1"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "1b3a99a110ef5cc8d64f6a3d6ac0a8b3467104f1c29d306eb0293d563e962034",
        "invalid_witnesses_utxow"
    ) =>
        matches Err(UTxOWValidationError::InvalidWitnessesUTxOW { key_hash, .. })
        if key_hash == KeyHash::from_str("6c7157fc2a0a260b9789bbec5a667d4d3f798848452f2113d10eabed").unwrap();
        "invalid_witnesses_utxow"
    )]
    #[allow(clippy::result_large_err)]
    fn conway_test((_ctx, raw_tx): (TestContext, Vec<u8>)) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(PallasEra::Conway, &raw_tx).unwrap();
        let mtx = tx.as_conway().unwrap();
        let vkey_witnesses = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses()).0;
        let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());
        validate(
            mtx,
            TxHash::from(*tx.hash()),
            &vkey_witnesses,
            &native_scripts,
        )
        .map_err(|e| *e)
    }
}
