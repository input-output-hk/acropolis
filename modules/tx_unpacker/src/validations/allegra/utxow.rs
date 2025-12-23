//! Allegra era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/allegra/impl/src/Cardano/Ledger/Allegra/Rules/Utxow.hs#L75
//!
//! NOTE: Allegra UTxOW rules are the same as Shelley UTxOW rules.

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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{test_utils::TestContext, validation_fixture};
    use acropolis_common::KeyHash;
    use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "allegra",
        "aee87cc55b9a4254497d2b2ea07981f32fd2cf0e1b4f94349a8c23f3d39eb576"
    ) =>
        matches Ok(());
        "valid transaction 1"
    )]
    #[test_case(validation_fixture!(
        "allegra",
        "fabfad0aaa2b52b8304f45edc0350659ad0d73f9d1065d9cd3ef7d5a599ac57d"
    ) =>
        matches Ok(());
        "valid transaction 2 - with mir certificates"
    )]
    #[test_case(validation_fixture!(
        "allegra",
        "aee87cc55b9a4254497d2b2ea07981f32fd2cf0e1b4f94349a8c23f3d39eb576",
        "invalid_witnesses_utxow"
    ) =>
        matches Err(UTxOWValidationError::InvalidWitnessesUTxOW { key_hash, .. })
        if key_hash == KeyHash::from_str("6a27b4eec5817b3f6c6af704c8936f2a6505c208e8c4933fdc154a08").unwrap();
        "invalid_witnesses_utxow"
    )]
    #[test_case(validation_fixture!(
        "allegra",
        "fabfad0aaa2b52b8304f45edc0350659ad0d73f9d1065d9cd3ef7d5a599ac57d",
        "mir_insufficient_genesis_sigs_utxow"
    ) =>
        matches Err(UTxOWValidationError::MIRInsufficientGenesisSigsUTXOW { genesis_keys, quorum: 5 })
        if genesis_keys.len() == 4;
        "mir_insufficient_genesis_sigs_utxow - 4 genesis sigs"
    )]
    #[allow(clippy::result_large_err)]
    fn allegra_test((ctx, raw_tx): (TestContext, Vec<u8>)) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(PallasEra::Allegra, &raw_tx).unwrap();
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
