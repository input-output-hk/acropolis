use std::collections::HashSet;

use acropolis_common::{
    validation::{Phase1ValidationError, TransactionValidationError},
    KeyHash, ScriptHash, UTXOValue, UTxOIdentifier,
};
use anyhow::Result;
mod shelley;

#[allow(dead_code)]
pub fn validate_shelley_tx<F>(
    inputs: &[UTxOIdentifier],
    vkey_hashes_needed: &mut HashSet<KeyHash>,
    script_hashes_needed: &mut HashSet<ScriptHash>,
    vkey_hashes_provided: &[KeyHash],
    script_hashes_provided: &[ScriptHash],
    lookup_utxo: F,
) -> Result<(), TransactionValidationError>
where
    F: Fn(&UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    shelley::utxo::validate(inputs, &lookup_utxo)
        .map_err(|e| Phase1ValidationError::UTxOValidationError(*e))?;
    shelley::utxow::validate(
        inputs,
        vkey_hashes_needed,
        script_hashes_needed,
        vkey_hashes_provided,
        script_hashes_provided,
        &lookup_utxo,
    )
    .map_err(|e| Phase1ValidationError::UTxOWValidationError(*e))?;

    Ok(())
}
