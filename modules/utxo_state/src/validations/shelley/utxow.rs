//! Shelley era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278
use acropolis_common::{validation::UTxOWValidationError, KeyHash, ScriptHash};
use anyhow::Result;
use std::collections::HashSet;

/// Validate that all required witnesses are provided
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L424
pub fn validate_needed_witnesses(
    vkey_hashes_needed: &HashSet<KeyHash>,
    witness_vkey_hashes: &HashSet<KeyHash>,
) -> Result<(), Box<UTxOWValidationError>> {
    let missing_vkey_hashes =
        vkey_hashes_needed.difference(witness_vkey_hashes).copied().collect::<HashSet<_>>();

    if let Some(vkey_hash) = missing_vkey_hashes.iter().next() {
        return Err(Box::new(UTxOWValidationError::MissingVKeyWitnessesUTxOW {
            key_hash: *vkey_hash,
        }));
    }
    Ok(())
}

/// Validate all needed scripts are provided in witnesses
/// No missing, no extra
/// Reference: Shelley: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L386
/// Babbage: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/Utxow.hs#L211
///
/// script_hashes_provided: scripts provided by the transaction (witnesseses + reference inputs)
/// script_hashes_needed: scripts needed for the transaction
/// witness_script_hashes: scripts provided by transaction witnesses
///
/// NOTE:
/// Scripts provided by reference inputs are not considered as Extra
pub fn validate_missing_extra_scripts(
    script_hashes_needed: &HashSet<ScriptHash>,
    script_witness_hashes: &HashSet<ScriptHash>,
    script_hashes_provided: &HashSet<ScriptHash>,
) -> Result<(), Box<UTxOWValidationError>> {
    // missing = needed - script_hashes_provided
    let missing_scripts =
        script_hashes_needed.difference(script_hashes_provided).copied().collect::<HashSet<_>>();
    if let Some(script_hash) = missing_scripts.iter().next() {
        return Err(Box::new(
            UTxOWValidationError::MissingScriptWitnessesUTxOW {
                script_hash: *script_hash,
            },
        ));
    }

    // extra = witness_script_hashes - needed
    // Reference scripts are not considered as Extra
    let extra_scripts =
        script_witness_hashes.difference(script_hashes_needed).copied().collect::<HashSet<_>>();
    if let Some(script_hash) = extra_scripts.iter().next() {
        return Err(Box::new(
            UTxOWValidationError::ExtraneousScriptWitnessesUTXOW {
                script_hash: *script_hash,
            },
        ));
    }

    Ok(())
}

pub fn validate(
    vkey_hashes_needed: &HashSet<KeyHash>,
    script_hashes_needed: &HashSet<ScriptHash>,
    vkey_witness_hashes: &HashSet<KeyHash>,
    script_witness_hashes: &HashSet<ScriptHash>,
    script_hashes_provided: &HashSet<ScriptHash>,
) -> Result<(), Box<UTxOWValidationError>> {
    // validate missing vkey witnesses
    validate_needed_witnesses(vkey_hashes_needed, vkey_witness_hashes)?;

    // validate missing & extra scripts
    validate_missing_extra_scripts(
        script_hashes_needed,
        script_witness_hashes,
        script_hashes_provided,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{test_utils::TestContext, utils, validation_fixture};
    use acropolis_common::{Era, NetworkId, TxIdentifier};
    use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "shelley",
        "da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b"
    ) =>
        matches Ok(());
        "valid transaction 1"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "b516588da34b58b7d32b6a057f513e16ea8c87de46615631be3316d8a8847d46"
    ) =>
        matches Ok(());
        "valid transaction 2 - with protocol update"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b",
        "missing_vkey_witnesses_utxow"
    ) =>
        matches Err(UTxOWValidationError::MissingVKeyWitnessesUTxOW { key_hash })
        if key_hash == KeyHash::from_str("b0baefb8dedefd7ec935514696ea5a66e9520f31dc8867737f0f0084").unwrap();
        "missing_vkey_witnesses_utxow"
    )]
    #[allow(clippy::result_large_err)]
    fn shelley_test((ctx, raw_tx): (TestContext, Vec<u8>)) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(PallasEra::Shelley, &raw_tx).unwrap();
        let raw_tx = tx.encode();
        let tx_identifier = TxIdentifier::new(4533644, 1);
        let mapped_tx = acropolis_codec::map_transaction(
            &tx,
            &raw_tx,
            tx_identifier,
            NetworkId::Mainnet,
            Era::Shelley,
        );
        let tx_error = mapped_tx.error.as_ref();
        assert!(tx_error.is_none());

        let tx_deltas = mapped_tx.convert_to_utxo_deltas(true);
        let vkey_hashes_needed =
            utils::get_vkeys_needed(&tx_deltas, &ctx.utxos, Some(&ctx.shelley_params));
        let scripts_needed = utils::get_scripts_needed(&tx_deltas, &ctx.utxos);
        let script_hashes_needed = scripts_needed.values().cloned().collect::<HashSet<_>>();

        let vkey_witness_hashes = tx_deltas.get_vkey_witness_hashes();
        let script_witness_hashes = tx_deltas.get_script_witness_hashes();

        let scripts_provided = utils::get_scripts_provided(&tx_deltas, &ctx.utxos);
        let script_hashes_provided = scripts_provided.keys().copied().collect::<HashSet<_>>();

        validate(
            &vkey_hashes_needed,
            &script_hashes_needed,
            &vkey_witness_hashes,
            &script_witness_hashes,
            &script_hashes_provided,
        )
        .map_err(|e| *e)
    }
}
