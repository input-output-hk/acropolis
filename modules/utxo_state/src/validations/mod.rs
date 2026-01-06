use std::collections::{HashMap, HashSet};

use acropolis_common::{
    validation::{Phase1ValidationError, TransactionValidationError},
    KeyHash, ScriptHash, UTXOValue, UTxOIdentifier, Value,
};
use anyhow::Result;
mod shelley;

#[allow(clippy::too_many_arguments)]
pub fn validate_shelley_tx(
    inputs: &[UTxOIdentifier],
    total_consumed_except_inputs: Value,
    total_produced: Value,
    vkey_hashes_needed: &mut HashSet<KeyHash>,
    script_hashes_needed: &mut HashSet<ScriptHash>,
    vkey_hashes_provided: &[KeyHash],
    script_hashes_provided: &[ScriptHash],
    utxos_needed: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), Box<TransactionValidationError>> {
    shelley::utxo::validate(
        inputs,
        total_consumed_except_inputs,
        total_produced,
        utxos_needed,
    )
    .map_err(|e| Box::new((Phase1ValidationError::UTxOValidationError(*e)).into()))?;
    shelley::utxow::validate(
        inputs,
        vkey_hashes_needed,
        script_hashes_needed,
        vkey_hashes_provided,
        script_hashes_provided,
        utxos_needed,
    )
    .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;

    Ok(())
}
