#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use acropolis_common::{
    genesis_values::GenesisValues, validation::Phase2ValidationError, CostModels, RedeemerPointer,
    ReferenceScript, ScriptHash, ScriptLang, TxUTxODeltas, UTXOValue, UTxOIdentifier,
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
#[allow(clippy::too_many_arguments)]
pub fn validate_tx_phase_two(
    tx_deltas: &TxUTxODeltas,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    genesis_values: &GenesisValues,
    cost_models: &CostModels,
    scripts_needed: &HashMap<RedeemerPointer, ScriptHash>,
    scripts_provided: &HashMap<ScriptHash, ScriptLang>,
    lookup_reference_script: &dyn Fn(&ScriptHash) -> Option<Arc<ReferenceScript>>,
) -> Result<(), Phase2ValidationError> {
    let scripts_table = build_scripts_table(tx_deltas, utxos, lookup_reference_script);

    let tx_info = TxInfo::new(tx_deltas, utxos, genesis_values)?;

    let script_contexts = build_script_contexts(&tx_info, scripts_needed, scripts_provided)?;

    evaluate_scripts(
        &script_contexts,
        &scripts_table,
        cost_models,
        tx_deltas.is_valid,
    )
}
