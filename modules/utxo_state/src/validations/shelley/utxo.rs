use std::collections::HashMap;

use acropolis_common::{validation::UTxOValidationError, UTXOValue, UTxOIdentifier, Value};
use anyhow::Result;

pub type UTxOValidationResult = Result<(), Box<UTxOValidationError>>;

/// Validate every transaction's input exists in the current UTxO set.
/// This prevents double spending.
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L468
pub fn validate_bad_inputs_utxo(
    inputs: &[UTxOIdentifier],
    utxos_needed: &HashMap<UTxOIdentifier, UTXOValue>,
) -> UTxOValidationResult {
    for (index, input) in inputs.iter().enumerate() {
        if utxos_needed.contains_key(input) {
            continue;
        } else {
            return Err(Box::new(UTxOValidationError::BadInputsUTxO {
                bad_input: *input,
                bad_input_index: index,
            }));
        }
    }
    Ok(())
}

pub fn validate_value_not_conserved(
    total_consumed: Value,
    total_produced: Value,
) -> UTxOValidationResult {
    if total_consumed != total_produced {
        return Err(Box::new(UTxOValidationError::ValueNotConservedUTxO {
            consumed: total_consumed,
            produced: total_produced,
        }));
    }
    Ok(())
}

pub fn validate(
    inputs: &[UTxOIdentifier],
    total_consumed: Value,
    total_produced: Value,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> UTxOValidationResult {
    validate_bad_inputs_utxo(inputs, utxos)?;

    validate_value_not_conserved(total_consumed, total_produced)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_utils::{to_era, to_pallas_era, TestContext},
        validation_fixture,
    };
    use acropolis_common::{NetworkId, TxHash, TxIdentifier, UTxOIdentifier};
    use pallas::ledger::traverse::MultiEraTx;
    use std::str::FromStr;
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "shelley",
        "da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b"
    ) =>
        matches Ok(());
        "shelley - valid transaction 1"
    )]
    #[test_case(validation_fixture!(
        "mary",
        "12eb4f1d4bc1dae27d916d4bc1a6cf6fd167c7413e77792fbd1edead310847ab"
    ) =>
        matches Ok(());
        "mary - valid transaction 1 - with native assets amount of 0"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "a6ae48485ace169cfac1dd7383fa8f26016c49637bab901215e9ee69c25313bd"
    ) =>
        matches Ok(());
        "conway - valid transaction 1 - failed transaction"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b",
        "bad_inputs_utxo"
    ) =>
        matches Err(UTxOValidationError::BadInputsUTxO { bad_input, bad_input_index })
        if bad_input == UTxOIdentifier::new(
            TxHash::from_str("e7075bff082ee708dfe49a366717dd4c6d51e9b3a7e5a070dcee253affda0999").unwrap(), 1)
            && bad_input_index == 0;
        "shelley - bad_inputs_utxo"
    )]
    #[test_case(validation_fixture!(
        "mary",
        "12eb4f1d4bc1dae27d916d4bc1a6cf6fd167c7413e77792fbd1edead310847ab",
        "value_not_conserved_utxo"
    ) =>
        matches Err(UTxOValidationError::ValueNotConservedUTxO { .. });
        "mary - value_not_conserved_utxo"
    )]
    #[allow(clippy::result_large_err)]
    fn shelley_utxo_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), UTxOValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let tx_inputs = acropolis_codec::map_transaction_inputs(&tx.consumes());
        let mapped_tx = acropolis_codec::map_transaction(
            &tx,
            &raw_tx,
            TxIdentifier::default(),
            NetworkId::Mainnet,
            to_era(era),
        );
        let tx_delta = mapped_tx.convert_to_utxo_deltas(true);
        let total_consumed = tx_delta.calculate_total_consumed(&[], &ctx.utxos);
        let total_produced = tx_delta.calculate_total_produced(&[], &[]);

        validate(&tx_inputs, total_consumed, total_produced, &ctx.utxos).map_err(|e| *e)
    }
}
