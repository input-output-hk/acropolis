use std::collections::HashMap;

use acropolis_common::{
    validation::{Phase1ValidationError, TransactionValidationError},
    PoolCertificateDelta, StakeCertificateDelta, TxUTxODeltas, UTXOValue, UTxOIdentifier, Value,
};
use anyhow::Result;
mod shelley;

pub fn validate_shelley_tx(
    tx_deltas: &TxUTxODeltas,
    pool_certificates_deltas: &[PoolCertificateDelta],
    stake_certificates_deltas: &[StakeCertificateDelta],
    utxos_needed: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Result<(), Box<TransactionValidationError>> {
    let inputs = &tx_deltas.consumes;

    // Consumed except inputs = Refund + Withrawals + Value Minted
    let total_refund = tx_deltas.calculate_total_refund(stake_certificates_deltas);
    let total_withdrawals = tx_deltas.total_withdrawals.unwrap_or_default();
    let mut total_consumed_except_inputs = Value::new(total_refund + total_withdrawals, vec![]);
    total_consumed_except_inputs += tx_deltas.value_minted.as_ref().unwrap_or(&Value::default());

    // Produced = Outputs + Fee + Deposits + Value Burnt
    let mut total_produced =
        tx_deltas.calculate_total_produced(pool_certificates_deltas, stake_certificates_deltas);
    total_produced += tx_deltas.value_burnt.as_ref().unwrap_or(&Value::default());

    let mut vkey_hashes_needed = tx_deltas.vkey_hashes_needed.clone().unwrap_or_default();
    let mut script_hashes_needed = tx_deltas.script_hashes_needed.clone().unwrap_or_default();
    let vkey_hashes_provided = match tx_deltas.vkey_hashes_provided.as_ref() {
        Some(v) => v,
        None => &Vec::new(),
    };
    let script_hashes_provided = match tx_deltas.script_hashes_provided.as_ref() {
        Some(v) => v,
        None => &Vec::new(),
    };

    shelley::utxo::validate(
        inputs,
        total_consumed_except_inputs,
        total_produced,
        utxos_needed,
    )
    .map_err(|e| Box::new((Phase1ValidationError::UTxOValidationError(*e)).into()))?;
    shelley::utxow::validate(
        inputs,
        &mut vkey_hashes_needed,
        &mut script_hashes_needed,
        vkey_hashes_provided,
        script_hashes_provided,
        utxos_needed,
    )
    .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;

    Ok(())
}
