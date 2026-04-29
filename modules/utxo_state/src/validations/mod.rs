use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use acropolis_common::{
    genesis_values::GenesisValues,
    protocol_params::ProtocolParams,
    validation::{Phase1ValidationError, ScriptEvaluationOutcome, TransactionValidationError},
    CostModels, Era, PoolRegistrationUpdate, ReferenceScript, ScriptHash, StakeRegistrationUpdate,
    TxUTxODeltas, UTXOValue, UTxOIdentifier,
};
use anyhow::Result;

use crate::utils;
mod alonzo;
mod babbage;
pub mod phase2;
mod phase_two;
mod shelley;

/// Run all validation phases for a single transaction.
///
/// Returns a per-Plutus-script outcome vector (one entry per script context that
/// went through phase-2 evaluation; empty when phase-2 is skipped — pre-Alonzo
/// or no redeemers) alongside the aggregate validation `Result`.
#[allow(clippy::too_many_arguments)]
pub fn validate_tx(
    tx_deltas: &TxUTxODeltas,
    pool_registration_updates: &[PoolRegistrationUpdate],
    stake_registration_updates: &[StakeRegistrationUpdate],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    protocol_params: &ProtocolParams,
    genesis_values: &GenesisValues,
    cost_models: &CostModels,
    lookup_reference_script: &dyn Fn(&ScriptHash) -> Option<Arc<ReferenceScript>>,
    era: Era,
) -> (
    Vec<ScriptEvaluationOutcome>,
    Result<(), Box<TransactionValidationError>>,
) {
    let mut outcomes: Vec<ScriptEvaluationOutcome> = Vec::new();
    let result = (|| -> Result<(), Box<TransactionValidationError>> {
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
            babbage::utxow::validate(created_reference_scripts, protocol_params)
                .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
        }

        // Phase 2: Plutus script execution (if params provided and redeemers present)
        let has_redeemers = tx_deltas.redeemers.as_ref().is_some_and(|r| !r.is_empty());
        if has_redeemers && era >= Era::Alonzo {
            let protocol_version = protocol_params.protocol_version().ok_or_else(|| {
                Box::new(
                    (Phase1ValidationError::Other("Protocol version is not set".to_string()))
                        .into(),
                )
            })?;
            let protocol_major_version = protocol_version.major;

            let (phase2_outcomes, phase2_result) = phase_two::validate_tx_phase_two(
                tx_deltas,
                utxos,
                genesis_values,
                cost_models,
                protocol_major_version,
                &scripts_needed,
                &scripts_provided,
                lookup_reference_script,
            );
            outcomes = phase2_outcomes;
            phase2_result.map_err(|e| Box::new(e.into()))?;
        }

        Ok(())
    })();
    (outcomes, result)
}
