//! Shelley era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use acropolis_common::{
    validation::UTxOWValidationError, KeyHash, ScriptHash, ShelleyAddressPaymentPart, UTXOValue,
    UTxOIdentifier,
};
use anyhow::Result;

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
