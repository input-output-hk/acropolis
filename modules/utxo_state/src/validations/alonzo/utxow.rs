#![allow(dead_code)]

use acropolis_common::{
    validation::UTxOWValidationError, Datum, DatumHash, Redeemer, RedeemerPointer, ReferenceScript,
    ScriptHash, ScriptType, ShelleyAddressPaymentPart, TxOutput, UTXOValue, UTxOIdentifier,
};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

/// This function checks consumed UTxOs for its attached datum
/// For each spending UTxO locked by script
/// - If it has a DatumHash: collect the hash
/// - If it has NoDatum AND is PlutusV1/V2: Return UnspendableUTxONoDatumHash error
/// - If it has NoDatum AND is PlutusV3: OK (CIP-0069)
///
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L241
pub fn get_input_datum_hashes(
    inputs: &[UTxOIdentifier],
    scripts_provided: &[(ScriptHash, ReferenceScript)],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<HashSet<DatumHash>, Box<UTxOWValidationError>> {
    let scripts_provided = scripts_provided
        .iter()
        .map(|(script_hash, script)| (script_hash, script.get_script_type()))
        .collect::<HashMap<_, _>>();
    let mut input_hashes = HashSet::new();

    for (index, input) in inputs.iter().enumerate() {
        if let Some(utxo) = utxos.get(input) {
            if let Some(ShelleyAddressPaymentPart::ScriptHash(script_hash)) =
                utxo.address.get_payment_part()
            {
                if let Some(script_type) = scripts_provided.get(&script_hash) {
                    match utxo.datum {
                        None => {
                            // only PlutusV3 doesn't require datum
                            if script_type.cmp(&ScriptType::PlutusV3) == Ordering::Less {
                                return Err(Box::new(
                                    UTxOWValidationError::UnspendableUTxONoDatumHash {
                                        utxo_identifier: *input,
                                        input_index: index,
                                    },
                                ));
                            }
                        }
                        Some(Datum::Hash(datum_hash)) => {
                            input_hashes.insert(datum_hash);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(input_hashes)
}

/// This function returns the allowed datums hashes
/// from outputs (only DatumHash)
/// and reference inputs (only DatumHash) - NEW from Babbage
pub fn get_allowed_datum_hashes(
    outputs: &[TxOutput],
    ref_inputs: &[UTxOIdentifier],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> HashSet<DatumHash> {
    let mut allowed_datum_hashes = HashSet::new();
    for output in outputs.iter() {
        if let Some(Datum::Hash(datum_hash)) = output.datum {
            allowed_datum_hashes.insert(datum_hash);
        }
    }

    for ref_input in ref_inputs.iter() {
        if let Some(utxo) = utxos.get(ref_input) {
            if let Some(Datum::Hash(datum_hash)) = utxo.datum {
                allowed_datum_hashes.insert(datum_hash);
            }
        }
    }
    allowed_datum_hashes
}

/// Validate whether required datums are missing
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L230
pub fn validate_datums(
    inputs: &[UTxOIdentifier],
    outputs: &[TxOutput],
    ref_inputs: &[UTxOIdentifier],
    scripts_provided: &[(ScriptHash, ReferenceScript)],
    plutus_data: &HashMap<DatumHash, Vec<u8>>,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), Box<UTxOWValidationError>> {
    let input_datum_hashes = get_input_datum_hashes(inputs, scripts_provided, utxos)?;

    // All input datum hashes must have datums in plutus data
    for datum_hash in input_datum_hashes.iter() {
        if !plutus_data.contains_key(datum_hash) {
            return Err(Box::new(UTxOWValidationError::MissingRequiredDatums {
                datum_hash: *datum_hash,
            }));
        }
    }

    let allowed_datum_hashes = get_allowed_datum_hashes(outputs, ref_inputs, utxos);

    // Supplemental datums must be all allowed (this is for outputs and ref inputs)
    let tx_datum_hashes = plutus_data.keys().copied().collect::<HashSet<_>>();
    let supplemental_datum_hashes =
        tx_datum_hashes.difference(&input_datum_hashes).copied().collect::<HashSet<_>>();

    for datum_hash in supplemental_datum_hashes.iter() {
        if !allowed_datum_hashes.contains(datum_hash) {
            return Err(Box::new(
                UTxOWValidationError::NotAllowedSupplementalDatums {
                    datum_hash: *datum_hash,
                },
            ));
        }
    }

    Ok(())
}

/// THis function validates the redeemers
/// Every plutus script must have exactly one Redeemer
/// But native scripts don't need redeemers
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L263
pub fn validate_redeemers(
    scripts_needed: &HashMap<RedeemerPointer, ScriptHash>,
    redeemers: &[Redeemer],
) -> Result<(), Box<UTxOWValidationError>> {
    // Check all scripts needed have one redeemer
    let redeemers_provided =
        redeemers.iter().map(|redeemer| redeemer.redeemer_pointer()).collect::<HashSet<_>>();
    for redeemer_pointer in scripts_needed.keys() {
        if !redeemers_provided.contains(redeemer_pointer) {
            return Err(Box::new(UTxOWValidationError::MissingRedeemers {
                redeemer_pointer: redeemer_pointer.clone(),
            }));
        }
    }

    // Check extra redeemers
    let needed_redeemer_pointers = scripts_needed.keys().cloned().collect::<HashSet<_>>();
    for redeemer_pointer in redeemers_provided.iter() {
        if !needed_redeemer_pointers.contains(redeemer_pointer) {
            return Err(Box::new(UTxOWValidationError::ExtraRedeemers {
                redeemer_pointer: redeemer_pointer.clone(),
            }));
        }
    }

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
    scripts_provided: &[(ScriptHash, ReferenceScript)],
    plutus_data: &HashMap<DatumHash, Vec<u8>>,
    redeemers: &[Redeemer],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), Box<UTxOWValidationError>> {
    validate_datums(
        inputs,
        outputs,
        ref_inputs,
        scripts_provided,
        plutus_data,
        utxos,
    )?;

    validate_redeemers(scripts_needed, redeemers)?;

    Ok(())
}
