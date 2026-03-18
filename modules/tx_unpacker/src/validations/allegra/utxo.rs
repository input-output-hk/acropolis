use acropolis_common::{
    protocol_params::ProtocolParams, validation::UTxOValidationError, Slot, ValidityInterval,
};
use anyhow::Result;
use pallas::ledger::traverse::{MultiEraOutput, MultiEraTx};

use crate::validations::utils;

pub type UTxOValidationResult = Result<(), Box<UTxOValidationError>>;

/// Validate transaction's validity range
/// Current slot must be within the transaction's validity range.
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/allegra/impl/src/Cardano/Ledger/Allegra/Rules/Utxo.hs#L242
pub fn validate_validity_range(
    validity_interval: &ValidityInterval,
    current_slot: u64,
) -> UTxOValidationResult {
    if !validity_interval.contains(current_slot) {
        return Err(Box::new(UTxOValidationError::OutsideValidityIntervalUTxO {
            current_slot,
            validity_interval: validity_interval.clone(),
        }));
    }
    Ok(())
}

pub fn validate_output_too_big_utxo(
    outputs: &[MultiEraOutput],
    collateral_return: &Option<MultiEraOutput>,
    protocol_params: &ProtocolParams,
) -> UTxOValidationResult {
    let alonzo_params = protocol_params.alonzo.as_ref().ok_or_else(|| {
        Box::new(UTxOValidationError::Other(
            "Alonzo params are not set".to_string(),
        ))
    })?;

    let validate_output = |index: usize, output: &MultiEraOutput| {
        let value_size = utils::get_value_size_in_words(output);
        if value_size > alonzo_params.max_value_size as u64 {
            return Err(Box::new(UTxOValidationError::OutputTooBigUTxO {
                output_index: index,
                value_size,
                max_value_size: alonzo_params.max_value_size as u64,
            }));
        }
        Ok(())
    };

    for (index, output) in outputs.iter().enumerate() {
        validate_output(index, output)?;
    }
    if let Some(collateral_return) = collateral_return {
        validate_output(0, collateral_return)?;
    }
    Ok(())
}

pub fn validate(
    tx: &MultiEraTx,
    validity_interval: &ValidityInterval,
    protocol_params: &ProtocolParams,
    current_slot: Slot,
) -> UTxOValidationResult {
    validate_validity_range(validity_interval, current_slot)?;
    validate_output_too_big_utxo(&tx.outputs(), &tx.collateral_return(), protocol_params)?;

    Ok(())
}
