use std::collections::{HashMap, HashSet};

use acropolis_common::{
    validation::{Phase1ValidationError, TransactionValidationError},
    KeyHash, ScriptHash, UTXOValue, UTxOIdentifier,
};
use anyhow::Result;
mod shelley;

#[allow(dead_code)]
pub fn validate_shelley_tx(
    inputs: &[UTxOIdentifier],
    vkey_hashes_needed: &mut HashSet<KeyHash>,
    script_hashes_needed: &mut HashSet<ScriptHash>,
    vkey_hashes_provided: &[KeyHash],
    script_hashes_provided: &[ScriptHash],
    utxos_needed: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), TransactionValidationError> {
    shelley::utxo::validate(inputs, utxos_needed)
        .map_err(|e| Phase1ValidationError::UTxOValidationError(*e))?;
    shelley::utxow::validate(
        inputs,
        vkey_hashes_needed,
        script_hashes_needed,
        vkey_hashes_provided,
        script_hashes_provided,
        utxos_needed,
    )
    .map_err(|e| Phase1ValidationError::UTxOWValidationError(*e))?;

    Ok(())
}
