#![allow(dead_code)]

use acropolis_common::{
    validation::UTxOWValidationError, Datum, DatumHash, Redeemer, RedeemerPointer, ScriptHash,
    ScriptLang, ShelleyAddressPaymentPart, TxOutput, UTXOValue, UTxOIdentifier,
};
use std::collections::{HashMap, HashSet};

/// This function checks consumed UTxOs for its attached datum
/// For each spending UTxO locked by script
/// - If it has a DatumHash: collect the hash
/// - If it has NoDatum AND is PlutusV1/V2: Return UnspendableUTxONoDatumHash error
/// - If it has NoDatum AND is PlutusV3: OK (CIP-0069)
///
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L241
pub fn get_input_datum_hashes(
    inputs: &[UTxOIdentifier],
    scripts_provided: &HashMap<ScriptHash, Option<ScriptLang>>,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<HashSet<DatumHash>, Box<UTxOWValidationError>> {
    let mut input_hashes = HashSet::new();

    for (index, input) in inputs.iter().enumerate() {
        if let Some(utxo) = utxos.get(input) {
            if let Some(ShelleyAddressPaymentPart::ScriptHash(script_hash)) =
                utxo.address.get_payment_part()
            {
                if let Some(Some(script_lang)) = scripts_provided.get(&script_hash) {
                    match utxo.datum {
                        None => {
                            // only PlutusV3 doesn't require datum
                            if !script_lang.eq(&ScriptLang::PlutusV3) {
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
    scripts_provided: &HashMap<ScriptHash, Option<ScriptLang>>,
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
    scripts_provided: &HashMap<ScriptHash, Option<ScriptLang>>,
    redeemers: &[Redeemer],
) -> Result<(), Box<UTxOWValidationError>> {
    let redeemers_needed = scripts_needed
        .iter()
        .filter(|(_, script_hash)| {
            scripts_provided
                .get(*script_hash)
                .map(|script_type| script_type.is_some())
                .unwrap_or(false)
        })
        .map(|(ptr, hash)| (ptr.clone(), *hash))
        .collect::<HashMap<_, _>>();

    // Check all scripts needed have one redeemer
    let redeemers_provided =
        redeemers.iter().map(|redeemer| redeemer.redeemer_pointer()).collect::<HashSet<_>>();
    for redeemer_pointer in redeemers_needed.keys() {
        if !redeemers_provided.contains(redeemer_pointer) {
            return Err(Box::new(UTxOWValidationError::MissingRedeemers {
                redeemer_pointer: redeemer_pointer.clone(),
            }));
        }
    }

    // Check extra redeemers
    let needed_redeemer_pointers = redeemers_needed.keys().cloned().collect::<HashSet<_>>();
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
    scripts_provided: &HashMap<ScriptHash, Option<ScriptLang>>,
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

    validate_redeemers(scripts_needed, scripts_provided, redeemers)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{test_utils::TestContext, utils, validation_fixture};
    use acropolis_common::{Era, NetworkId, RedeemerTag, TxHash, TxIdentifier};
    use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "alonzo",
        "de5a43595e3257b9cccb90a396c455a0ed3895a7d859fb507b85363ee4638590"
    ) =>
        matches Ok(());
        "alonzo - valid transaction 1 - with contracts"
    )]
    #[test_case(validation_fixture!(
        "alonzo",
        "de5a43595e3257b9cccb90a396c455a0ed3895a7d859fb507b85363ee4638590",
        "missing_redeemers"
    ) =>
        matches Err(UTxOWValidationError::MissingRedeemers { redeemer_pointer })
        if redeemer_pointer == RedeemerPointer {tag: RedeemerTag::Spend, index: 1};
        "alonzo - missing redeemers"
    )]
    #[test_case(validation_fixture!(
        "alonzo",
        "de5a43595e3257b9cccb90a396c455a0ed3895a7d859fb507b85363ee4638590",
        "extra_redeemers"
    ) =>
        matches Err(UTxOWValidationError::ExtraRedeemers { redeemer_pointer })
        if redeemer_pointer == RedeemerPointer {tag: RedeemerTag::Spend, index: 0};
        "alonzo - extra redeemers"
    )]
    #[test_case(validation_fixture!(
        "alonzo",
        "de5a43595e3257b9cccb90a396c455a0ed3895a7d859fb507b85363ee4638590",
        "missing_required_datums"
    ) =>
        matches Err(UTxOWValidationError::MissingRequiredDatums { datum_hash })
        if datum_hash == DatumHash::from_str("c8296567eaffef4efdafa652335bbd34e91cddbcf061a17d110d16e540324c32").unwrap();
        "alonzo - missing required datums"
    )]
    #[test_case(validation_fixture!(
        "alonzo",
        "de5a43595e3257b9cccb90a396c455a0ed3895a7d859fb507b85363ee4638590",
        "not_allowed_supplemental_datums"
    ) =>
        matches Err(UTxOWValidationError::NotAllowedSupplementalDatums { datum_hash })
        if datum_hash == DatumHash::from_str("f1f6589679d8b007a9a83c71b0e4450202dbebb897d296597fb218633d102a5e").unwrap();
        "alonzo - not allowed supplemental datums"
    )]
    #[test_case(validation_fixture!(
        "alonzo",
        "de5a43595e3257b9cccb90a396c455a0ed3895a7d859fb507b85363ee4638590",
        "unspendable_utxo_no_datum_hash"
    ) =>
        matches Err(UTxOWValidationError::UnspendableUTxONoDatumHash { utxo_identifier, input_index })
        if utxo_identifier == UTxOIdentifier {
            tx_hash: TxHash::from_str("241f6fa120e4c2d28282553f6116c4bb3bb3b14e42c047493b492be656b8f41a").unwrap(), 
            output_index: 2u16
        } && input_index == 0;
        "alonzo - unspendable utxo no datum hash"
    )]
    #[allow(clippy::result_large_err)]
    fn alonzo_test((ctx, raw_tx): (TestContext, Vec<u8>)) -> Result<(), UTxOWValidationError> {
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
        let inputs = &tx_deltas.consumes;
        let outputs = &tx_deltas.produces;
        let ref_inputs = &tx_deltas.reference_inputs;
        let plutus_data = &tx_deltas.plutus_data.clone().unwrap_or_default();
        let redeemers = &tx_deltas.redeemers.clone().unwrap_or_default();
        let scripts_needed = utils::get_scripts_needed(&tx_deltas, &ctx.utxos);
        let scripts_provided = utils::get_scripts_provided(&tx_deltas, &ctx.utxos);

        validate(
            inputs,
            outputs,
            ref_inputs,
            &scripts_needed,
            &scripts_provided,
            plutus_data,
            redeemers,
            &ctx.utxos,
        )
        .map_err(|e| *e)
    }
}
