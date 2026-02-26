use std::collections::HashMap;

use acropolis_common::{validation::UTxOValidationError, UTXOValue, UTxOIdentifier, ValueMap};
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
    total_consumed: ValueMap,
    total_produced: ValueMap,
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
    total_consumed: ValueMap,
    total_produced: ValueMap,
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
        "alonzo",
        "f9ed2fef27cdcf60c863ba03f27d0e38f39c5047cf73ffdf2428b48edbe83234"
    ) =>
        matches Ok(());
        "alonzo - valid transaction 1 - failed transaction"
    )]
    #[test_case(validation_fixture!(
        "babbage",
        "b2d01aec0fc605e699b1145d8ff9fce132a9108c8e026177ce648ddbe79473b5"
    ) =>
        matches Ok(());
        "babbage - valid transaction 1 - failed transaction with collateral return"
    )]
    #[test_case(validation_fixture!(
        "babbage",
        "0104b80dd6061ee0a452d612fb608c43149033ef34c622ae634d579bd4fc3892"
    ) =>
        matches Ok(());
        "babbage - valid transaction 2 - failed transaction without collateral return"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "fd7ba73b225deafa5157ad8475802bed4e15a26e0376298d1ce37574acbb6527"
    ) =>
        matches Ok(());
        "conway - valid transaction 1 - transaction with DRep Registration Certificate"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "7fd6429add8f2611ad8d48c0cc49101463093aec285faea402e8cfde78ea58d7"
    ) =>
        matches Ok(());
        "conway - valid transaction 2 - transaction with Governance Proposal"
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
        let total_produced = tx_delta.calculate_total_produced(&[], &[], &ctx.utxos);

        validate(&tx_inputs, total_consumed, total_produced, &ctx.utxos).map_err(|e| *e)
    }
}
