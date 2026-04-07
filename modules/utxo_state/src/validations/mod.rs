use std::collections::{HashMap, HashSet};

use acropolis_common::{
    protocol_params::ProtocolParams,
    validation::{Phase1ValidationError, TransactionValidationError},
    Era, PoolRegistrationUpdate, ReferenceScript, ScriptHash, StakeRegistrationUpdate,
    TxUTxODeltas, UTXOValue, UTxOIdentifier,
};
use anyhow::Result;

use crate::utils;
mod alonzo;
mod babbage;
pub mod phase2;
mod phase_two;
mod shelley;

pub fn validate_tx(
    tx_deltas: &TxUTxODeltas,
    pool_registration_updates: &[PoolRegistrationUpdate],
    stake_registration_updates: &[StakeRegistrationUpdate],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    protocol_params: &ProtocolParams,
    era: Era,
) -> Result<(), Box<TransactionValidationError>> {
    let inputs = &tx_deltas.consumes;
    let total_consumed = tx_deltas.calculate_total_consumed(stake_registration_updates, utxos);
    let total_produced = tx_deltas.calculate_total_produced(
        pool_registration_updates,
        stake_registration_updates,
        utxos,
    );

    let vkey_hashes_needed =
        utils::get_vkeys_needed(tx_deltas, utxos, protocol_params.genesis_delegates());
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
            tx_deltas.is_valid,
        )
        .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
    }

    if era >= Era::Alonzo {
        let outputs = &tx_deltas.produces;
        let ref_inputs = &tx_deltas.reference_inputs;
        let plutus_data = &tx_deltas.plutus_data.clone().unwrap_or_default();
        let redeemers = &tx_deltas.redeemers.clone().unwrap_or_default();
        alonzo::utxow::validate(
            inputs,
            outputs,
            ref_inputs,
            &scripts_needed,
            &scripts_provided,
            plutus_data,
            redeemers,
            utxos,
            tx_deltas.is_valid,
        )
        .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
    }

    if era >= Era::Babbage {
        let created_reference_scripts: HashMap<ScriptHash, &ReferenceScript> = tx_deltas
            .created_reference_scripts
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|(hash, script)| (*hash, script))
            .collect();
        babbage::utxow::validate(created_reference_scripts)
            .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
    }

    Ok(())
}
