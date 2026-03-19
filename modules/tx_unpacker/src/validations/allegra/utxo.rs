use acropolis_common::{
    protocol_params::ProtocolParams, validation::UTxOValidationError, Era, Slot, ValidityInterval,
};
use anyhow::Result;
use pallas::ledger::traverse::{MultiEraOutput, MultiEraTx};

use crate::validations::utils;

pub type UTxOValidationResult = Result<(), Box<UTxOValidationError>>;

const ALLEGRA_MAX_VALUE_SIZE: u64 = 4000;

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

/// Validate output's value size is not too big
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/allegra/impl/src/Cardano/Ledger/Allegra/Rules/Utxo.hs#L254
pub fn validate_output_too_big_utxo(
    outputs: &[MultiEraOutput],
    collateral_return: &Option<MultiEraOutput>,
    protocol_params: &ProtocolParams,
    era: Era,
) -> UTxOValidationResult {
    let max_value_size = if era < Era::Allegra {
        unreachable!("This check should be called since Allegra era");
    } else if era == Era::Allegra {
        ALLEGRA_MAX_VALUE_SIZE
    } else {
        protocol_params.max_value_size().ok_or_else(|| {
            Box::new(UTxOValidationError::Other(
                "Alonzo params are not set".to_string(),
            ))
        })?
    };

    let validate_output = |index: usize, output: &MultiEraOutput| {
        let value_size = utils::get_value_size_in_bytes(output);
        if value_size > max_value_size {
            return Err(Box::new(UTxOValidationError::OutputTooBigUTxO {
                output_index: index,
                value_size,
                max_value_size,
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
    era: Era,
) -> UTxOValidationResult {
    validate_validity_range(validity_interval, current_slot)?;
    validate_output_too_big_utxo(&tx.outputs(), &tx.collateral_return(), protocol_params, era)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        test_utils::{to_era, to_pallas_era, TestContext},
        validation_fixture,
    };
    use test_case::test_case;

    use super::*;

    #[test_case(validation_fixture!(
        "allegra",
        "2305653c3c37d1ab2e94a3c0b06ddaaf32db589e726bbde070dcbb1e764506d5"
    ) =>
        matches Ok(());
        "allegra - valid transaction 1"
    )]
    #[test_case(validation_fixture!(
        "allegra",
        "2305653c3c37d1ab2e94a3c0b06ddaaf32db589e726bbde070dcbb1e764506d5",
        "outside_validity_interval_utxo"
    ) =>
        matches Err(UTxOValidationError::OutsideValidityIntervalUTxO {
            current_slot,
            validity_interval,
        }) if current_slot == 23082605 && validity_interval == ValidityInterval::new(Some(23082606), Some(23164614));
        "allegra - outside validity interval utxo"
    )]
    #[test_case(validation_fixture!(
        "allegra",
        "2305653c3c37d1ab2e94a3c0b06ddaaf32db589e726bbde070dcbb1e764506d5",
        "output_too_big_utxo"
    ) =>
        matches Err(UTxOValidationError::OutputTooBigUTxO {
            output_index: 0,
            value_size: 6924,
            max_value_size: 4000,
        });
        "allegra - output too big utxo"
    )]
    #[allow(clippy::result_large_err)]
    fn allegra_utxo_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), UTxOValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let validity_interval = acropolis_codec::map_validity_interval(&tx);
        let era = to_era(era);

        validate(
            &tx,
            &validity_interval,
            &ctx.protocol_params,
            ctx.current_slot,
            era,
        )
        .map_err(|e| *e)
    }
}
