use crate::validations::shelley;
use acropolis_common::{
    validation::UTxOWValidationError, KeyHash, Redeemer, RedeemerPointer, ScriptHash, UTXOValue,
    UTxOIdentifier,
};
use std::collections::{HashMap, HashSet};

pub fn validate_redeemers(
    inputs: &[UTxOIdentifier],
    redeemers: &[Redeemer],
    utxos_needed: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), Box<UTxOWValidationError>> {
    Ok(())
}

/// 1. MissingRedeemers
/// 2. ExtraRedeemers
/// 3. MissingRequiredDatums
/// 4. NotAllowedSupplementalDatums
/// 5. UnspendableUTxONoDatumHash
pub fn validate(
    vkey_hashes_needed: &HashSet<KeyHash>,
    script_hashes_needed: &HashSet<ScriptHash>,
    vkey_hashes_provided: &[KeyHash],
    script_hashes_provided: &[ScriptHash],
    scripts_needed: &[(RedeemerPointer, ScriptHash)],
    redeemers: &[Redeemer],
) -> Result<(), Box<UTxOWValidationError>> {
    shelley::utxow::validate(
        vkey_hashes_needed,
        script_hashes_needed,
        vkey_hashes_provided,
        script_hashes_provided,
    )?;

    Ok(())
}
