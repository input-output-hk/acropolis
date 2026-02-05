use std::collections::{HashMap, HashSet};

use acropolis_common::{
    protocol_params::ShelleyParams,
    validation::{Phase1ValidationError, TransactionValidationError},
    Era, PoolRegistrationUpdate, StakeRegistrationUpdate, TxUTxODeltas, UTXOValue, UTxOIdentifier,
};
use anyhow::Result;

use crate::utils;
mod alonzo;
mod shelley;

pub fn validate_tx(
    tx_deltas: &TxUTxODeltas,
    pool_registration_updates: &[PoolRegistrationUpdate],
    stake_registration_updates: &[StakeRegistrationUpdate],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    shelley_params: Option<&ShelleyParams>,
    era: Era,
) -> Result<(), Box<TransactionValidationError>> {
    let inputs = &tx_deltas.consumes;
    let total_consumed = tx_deltas.calculate_total_consumed(stake_registration_updates, utxos);
    let total_produced =
        tx_deltas.calculate_total_produced(pool_registration_updates, stake_registration_updates);

    let vkey_hashes_needed = utils::get_vkeys_needed(tx_deltas, utxos, shelley_params);
    let scripts_needed = utils::get_scripts_needed(tx_deltas, utxos);
    let script_hashes_needed = scripts_needed.values().copied().collect::<HashSet<_>>();

    let vkey_witness_hashes = tx_deltas.get_vkey_witness_hashes();
    let script_witness_hashes = tx_deltas.get_script_witness_hashes();

    let scripts_provided = utils::get_scripts_provided(tx_deltas, utxos);
    let script_hashes_provided = scripts_provided.keys().copied().collect::<HashSet<_>>();

    if era >= Era::Shelley {
        shelley::utxo::validate(inputs, total_consumed, total_produced, utxos)
            .map_err(|e| Box::new((Phase1ValidationError::UTxOValidationError(*e)).into()))?;

        shelley::utxow::validate(
            &vkey_hashes_needed,
            &script_hashes_needed,
            &vkey_witness_hashes,
            &script_witness_hashes,
            &script_hashes_provided,
        )
        .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
    }

    Ok(())
}
