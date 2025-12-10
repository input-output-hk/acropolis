use acropolis_common::{validation::UTxOValidationError, UTXOValue, UTxOIdentifier};
use anyhow::Result;
use pallas::ledger::primitives::alonzo;

pub type UTxOValidationResult = Result<(), Box<UTxOValidationError>>;

/// Validate every transaction's input exists in the current UTxO set.
/// This prevents double spending.
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L468
pub fn validate_bad_inputs_utxo<F>(
    transaction_body: &alonzo::TransactionBody,
    lookup_utxo: F,
) -> UTxOValidationResult
where
    F: Fn(UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    for (index, input) in transaction_body.inputs.iter().enumerate() {
        let tx_ref = UTxOIdentifier::new((*input.transaction_id).into(), input.index as u16);
        if let Ok(Some(_)) = lookup_utxo(tx_ref) {
            continue;
        } else {
            return Err(Box::new(UTxOValidationError::BadInputsUTxO {
                bad_input: tx_ref,
                bad_input_index: index,
            }));
        }
    }
    Ok(())
}
