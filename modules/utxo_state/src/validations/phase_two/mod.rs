#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use acropolis_common::{
    genesis_values::GenesisValues,
    validation::{Phase2ValidationError, ScriptEvaluationOutcome},
    CostModels, RedeemerPointer, ReferenceScript, ScriptHash, ScriptLang, TxUTxODeltas, UTXOValue,
    UTxOIdentifier,
};

mod address;
mod cert;
mod evaluate;
mod governance;
mod input;
mod script_context;
mod time_range;
mod to_plutus_data;
mod utils;
mod value;

pub use evaluate::{build_scripts_table, evaluate_scripts};
pub use script_context::{build_script_contexts, TxInfo};

/// Run phase 2 Plutus script validation for a transaction.
///
/// Builds the transaction info, resolves all script contexts, and evaluates
/// each Plutus script in parallel. Reuses `scripts_needed` and `scripts_provided`
/// already computed during phase 1 validation.
///
/// Returns a per-script outcome vector (one entry per Plutus script context;
/// empty if there were no Plutus scripts) alongside the aggregate `Result`.
#[allow(clippy::too_many_arguments)]
pub fn validate_tx_phase_two(
    tx_deltas: &TxUTxODeltas,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    genesis_values: &GenesisValues,
    cost_models: &CostModels,
    protocol_major_version: u64,
    scripts_needed: &HashMap<RedeemerPointer, ScriptHash>,
    scripts_provided: &HashMap<ScriptHash, ScriptLang>,
    lookup_reference_script: &dyn Fn(&ScriptHash) -> Option<Arc<ReferenceScript>>,
) -> (
    Vec<ScriptEvaluationOutcome>,
    Result<(), Phase2ValidationError>,
) {
    let scripts_table = build_scripts_table(tx_deltas, utxos, lookup_reference_script);

    let tx_info = match TxInfo::new(tx_deltas, utxos, genesis_values) {
        Ok(ti) => ti,
        Err(e) => return (Vec::new(), Err(e.into())),
    };

    let script_contexts = match build_script_contexts(&tx_info, scripts_needed, scripts_provided) {
        Ok(scs) => scs,
        Err(e) => return (Vec::new(), Err(e.into())),
    };

    evaluate_scripts(
        &tx_info,
        &script_contexts,
        &scripts_table,
        cost_models,
        protocol_major_version,
        tx_deltas.is_valid,
    )
}
