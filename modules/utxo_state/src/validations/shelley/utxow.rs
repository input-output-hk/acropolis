//! Shelley era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278
use acropolis_common::{
    validation::UTxOWValidationError, KeyHash, ScriptHash, ShelleyAddressPaymentPart, UTXOValue,
    UTxOIdentifier,
};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

fn get_vkey_script_needed_from_inputs(
    inputs: &[UTxOIdentifier],
    vkey_hashes_needed: &mut HashSet<KeyHash>,
    script_hashes_needed: &mut HashSet<ScriptHash>,
    utxos_needed: &HashMap<UTxOIdentifier, UTXOValue>,
) {
    // for each UTxO, extract the needed vkey and script hashes
    for utxo in inputs.iter() {
        if let Some(utxo) = utxos_needed.get(utxo) {
            // NOTE:
            // Need to check inputs from byron bootstrap addresses
            // with bootstrap witnesses
            if let Some(payment_part) = utxo.address.get_payment_part() {
                match payment_part {
                    ShelleyAddressPaymentPart::PaymentKeyHash(payment_key_hash) => {
                        vkey_hashes_needed.insert(payment_key_hash);
                    }
                    ShelleyAddressPaymentPart::ScriptHash(script_hash) => {
                        script_hashes_needed.insert(script_hash);
                    }
                }
            }
        }
    }
}

/// Validate all needed scripts are provided in witnesses
/// No missing, no extra
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L386
pub fn validate_missing_extra_scripts(
    script_hashes_needed: &HashSet<ScriptHash>,
    script_hashes_provided: &[ScriptHash],
) -> Result<(), Box<UTxOWValidationError>> {
    let mut scripts_used = script_hashes_provided.iter().map(|h| (false, *h)).collect::<Vec<_>>();

    // check for missing & extra scripts
    for script_hash in script_hashes_needed.iter() {
        if let Some((used, _)) = scripts_used.iter_mut().find(|(u, h)| !(*u) && script_hash.eq(h)) {
            *used = true;
        } else {
            return Err(Box::new(
                UTxOWValidationError::MissingScriptWitnessesUTxOW {
                    script_hash: *script_hash,
                },
            ));
        }
    }

    for (used, script_hash) in scripts_used.iter() {
        if !*used {
            return Err(Box::new(
                UTxOWValidationError::ExtraneousScriptWitnessesUTXOW {
                    script_hash: *script_hash,
                },
            ));
        }
    }
    Ok(())
}

/// Validate that all required witnesses are provided
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L424
pub fn validate_needed_witnesses(
    vkey_hashes_needed: &HashSet<KeyHash>,
    vkey_hashes_provided: &[KeyHash],
) -> Result<(), Box<UTxOWValidationError>> {
    for vkey_hash in vkey_hashes_needed.iter() {
        if !vkey_hashes_provided.contains(vkey_hash) {
            return Err(Box::new(UTxOWValidationError::MissingVKeyWitnessesUTxOW {
                key_hash: *vkey_hash,
            }));
        }
    }
    Ok(())
}

pub fn validate(
    inputs: &[UTxOIdentifier],
    // Need to include vkey hashes and script hashes
    // from inputs
    vkey_hashes_needed: &mut HashSet<KeyHash>,
    script_hashes_needed: &mut HashSet<ScriptHash>,
    vkey_hashes_provided: &[KeyHash],
    script_hashes_provided: &[ScriptHash],
    utxos_needed: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), Box<UTxOWValidationError>> {
    // Extract vkey hashes and script hashes from inputs
    get_vkey_script_needed_from_inputs(
        inputs,
        vkey_hashes_needed,
        script_hashes_needed,
        utxos_needed,
    );

    // validate missing & extra scripts
    validate_missing_extra_scripts(script_hashes_needed, script_hashes_provided)?;

    // validate required vkey witnesses are provided
    validate_needed_witnesses(vkey_hashes_needed, vkey_hashes_provided)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{test_utils::TestContext, validation_fixture};
    use acropolis_common::{
        AlonzoBabbageUpdateProposal, Era, GenesisDelegates, NetworkId, Transaction,
        TxCertificateWithPos, TxIdentifier, Withdrawal,
    };
    use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
    use test_case::test_case;

    fn get_vkey_script_needed(
        certs: &[TxCertificateWithPos],
        withdrawals: &[Withdrawal],
        proposal_update: &Option<AlonzoBabbageUpdateProposal>,
        genesis_delgs: &GenesisDelegates,
        vkey_hashes: &mut HashSet<KeyHash>,
        script_hashes: &mut HashSet<ScriptHash>,
    ) {
        // for each certificate, get the required vkey and script hashes
        for cert_with_pos in certs.iter() {
            cert_with_pos.cert.get_cert_authors(vkey_hashes, script_hashes);
        }

        // for each withdrawal, get the required vkey and script hashes
        for withdrawal in withdrawals.iter() {
            withdrawal.get_withdrawal_authors(vkey_hashes, script_hashes);
        }

        // for each governance action, get the required vkey hashes
        if let Some(proposal_update) = proposal_update.as_ref() {
            proposal_update.get_governance_authors(vkey_hashes, genesis_delgs);
        }
    }

    #[test_case(validation_fixture!("da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b") =>
        matches Ok(());
        "valid transaction 1"
    )]
    #[test_case(validation_fixture!("b516588da34b58b7d32b6a057f513e16ea8c87de46615631be3316d8a8847d46") =>
        matches Ok(());
        "valid transaction 2 - with protocol update"
    )]
    #[test_case(validation_fixture!("da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b", "missing_vkey_witnesses_utxow") =>
        matches Err(UTxOWValidationError::MissingVKeyWitnessesUTxOW { key_hash })
        if key_hash == KeyHash::from_str("b0baefb8dedefd7ec935514696ea5a66e9520f31dc8867737f0f0084").unwrap();
        "missing_vkey_witnesses_utxow"
    )]
    #[allow(clippy::result_large_err)]
    fn shelley_test((ctx, raw_tx): (TestContext, Vec<u8>)) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(PallasEra::Shelley, &raw_tx).unwrap();
        let raw_tx = tx.encode();
        let tx_identifier = TxIdentifier::new(4533644, 1);
        let Transaction {
            consumes: tx_consumes,
            certs: tx_certs,
            withdrawals: tx_withdrawals,
            proposal_update: tx_proposal_update,
            vkey_witnesses,
            native_scripts,
            error: tx_error,
            ..
        } = acropolis_codec::map_transaction(
            &tx,
            &raw_tx,
            tx_identifier,
            NetworkId::Mainnet,
            Era::Shelley,
        );
        assert!(tx_error.is_none());

        let mut vkey_hashes_needed = HashSet::new();
        let mut script_hashes_needed = HashSet::new();
        get_vkey_script_needed(
            &tx_certs,
            &tx_withdrawals,
            &tx_proposal_update,
            &ctx.shelley_params.gen_delegs,
            &mut vkey_hashes_needed,
            &mut script_hashes_needed,
        );
        let vkey_hashes_provided = vkey_witnesses.iter().map(|w| w.key_hash()).collect::<Vec<_>>();
        let script_hashes_provided =
            native_scripts.iter().map(|s| s.compute_hash()).collect::<Vec<_>>();

        validate(
            &tx_consumes,
            &mut vkey_hashes_needed,
            &mut script_hashes_needed,
            &vkey_hashes_provided,
            &script_hashes_provided,
            &ctx.utxos,
        )
        .map_err(|e| *e)
    }
}
