//! Shelley era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278
#![allow(dead_code)]

use std::collections::HashSet;

use acropolis_common::{
    validation::UTxOWValidationError, GenesisDelegates, KeyHash, ScriptHash,
    ShelleyAddressPaymentPart, TxCertificate, TxCertificateWithPos, UTXOValue, UTxOIdentifier,
};
use anyhow::Result;

pub fn get_vkey_script_needed_from_inputs<F>(
    inputs: &[UTxOIdentifier],
    vkey_hashes_needed: &mut HashSet<KeyHash>,
    script_hashes_needed: &mut HashSet<ScriptHash>,
    lookup_utxo: F,
) where
    F: Fn(&UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    // for each UTxO, extract the needed vkey and script hashes
    for utxo in inputs.iter() {
        if let Ok(Some(utxo)) = lookup_utxo(utxo) {
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

/// Validate genesis keys signatures for MIR certificate
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L463
pub fn validate_mir_insufficient_genesis_sigs(
    certificates: &[TxCertificateWithPos],
    vkey_hashes_provided: &HashSet<KeyHash>,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
) -> Result<(), Box<UTxOWValidationError>> {
    let has_mir = certificates.iter().any(|cert_with_pos| {
        matches!(
            cert_with_pos.cert,
            TxCertificate::MoveInstantaneousReward(_)
        )
    });
    if !has_mir {
        return Ok(());
    }

    let genesis_delegate_hashes =
        genesis_delegs.as_ref().values().map(|delegate| delegate.delegate).collect::<HashSet<_>>();

    // genSig := genDelegates ∩ witsKeyHashes
    let genesis_sigs =
        genesis_delegate_hashes.intersection(vkey_hashes_provided).copied().collect::<HashSet<_>>();

    // Check: |genSig| ≥ Quorum
    // If insufficient, report the signatures that were found (not the missing ones)
    if genesis_sigs.len() < update_quorum as usize {
        return Err(Box::new(
            UTxOWValidationError::MIRInsufficientGenesisSigsUTXOW {
                genesis_keys: genesis_sigs,
                quorum: update_quorum,
            },
        ));
    }

    Ok(())
}

pub fn validate<F>(
    inputs: &[UTxOIdentifier],
    // Need to include vkey hashes and script hashes
    // from inputs
    vkey_hashes_needed: &mut HashSet<KeyHash>,
    script_hashes_needed: &mut HashSet<ScriptHash>,
    vkey_hashes_provided: &[KeyHash],
    script_hashes_provided: &[ScriptHash],
    lookup_utxo: F,
) -> Result<(), Box<UTxOWValidationError>>
where
    F: Fn(&UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    // Extract vkey hashes and script hashes from inputs
    get_vkey_script_needed_from_inputs(
        inputs,
        vkey_hashes_needed,
        script_hashes_needed,
        lookup_utxo,
    );

    // validate missing & extra scripts
    validate_missing_extra_scripts(script_hashes_needed, script_hashes_provided)?;

    // validate required vkey witnesses are provided
    validate_needed_witnesses(vkey_hashes_needed, vkey_hashes_provided)?;

    Ok(())
}
