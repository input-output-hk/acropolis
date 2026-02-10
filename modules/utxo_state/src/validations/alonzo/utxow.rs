#![allow(dead_code)]
#![allow(unused_variables)]
use acropolis_common::{
    validation::UTxOWValidationError, DatumHash, Redeemer, RedeemerPointer, ScriptHash, ScriptLang,
    TxOutput, UTXOValue, UTxOIdentifier,
};
use std::collections::{HashMap, HashSet};

/// This function checks consumed UTxOs for its attached datum
/// For each spending UTxO locked by script
/// - If it has a DatumHash: collect the hash
/// - If it has NoDatum AND is PlutusV1/V2: Return UnspendableUTxONoDatumHash error
/// - If it has NoDatum AND is PlutusV3: OK (CIP-0069)
///
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L241
pub fn get_input_datam_hashes(
    inputs: &[UTxOIdentifier],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<HashSet<DatumHash>, Box<UTxOWValidationError>> {
    Ok(HashSet::new())
}

pub fn validate_datums(
    inputs: &[UTxOIdentifier],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), Box<UTxOWValidationError>> {
    Ok(())
}

pub fn validate_redeemers(
    inputs: &[UTxOIdentifier],
    redeemers: &[Redeemer],
    utxos_needed: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), Box<UTxOWValidationError>> {
    Ok(())
}

/// There are new Alonzo UTxOW rules
/// 1. MissingRedeemers
/// 2. ExtraRedeemers
/// 3. MissingRequiredDatums
/// 4. NotAllowedSupplementalDatums
/// 5. UnspendableUTxONoDatumHash
#[allow(clippy::too_many_arguments)]
pub fn validate(
    inputs: &[UTxOIdentifier],
    outputs: &[TxOutput],
    ref_inputs: &[UTxOIdentifier],
    scripts_needed: &HashMap<RedeemerPointer, ScriptHash>,
    scripts_provided: &HashMap<ScriptHash, ScriptLang>,
    plutus_data: &HashMap<DatumHash, Vec<u8>>,
    redeemers: &[Redeemer],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), Box<UTxOWValidationError>> {
    Ok(())
}
