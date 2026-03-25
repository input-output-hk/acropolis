#![allow(unused_imports)]

use std::collections::HashMap;

use acropolis_common::{
    genesis_values::GenesisValues, CostModels, RedeemerPointer, ReferenceScript, ScriptHash,
    ScriptLang, TxUTxODeltas, UTXOValue, UTxOIdentifier,
};

pub mod execute;
pub mod to_plutus_data;

mod address;
mod cert;
mod governance;
mod input;
mod time_range;
mod value;

pub mod script_context;

pub use acropolis_common::validation::{Phase2ValidationError, ScriptContextError};
pub use execute::{build_scripts_table, evaluate_raw_flat_program, execute_scripts};
pub use input::ResolvedInput;
pub use script_context::{build_script_contexts, ScriptContext, TxInfo};
pub use time_range::TimeRange;
pub use to_plutus_data::ToPlutusData;

/// Run phase 2 Plutus script validation for a transaction.
///
/// Builds the transaction info, resolves all script contexts, and executes
/// each Plutus script in parallel. Reuses `scripts_needed` and `scripts_provided`
/// already computed during phase 1 validation.
pub fn validate_tx_phase2(
    tx_deltas: &TxUTxODeltas,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    genesis_values: &GenesisValues,
    cost_models: &CostModels,
    scripts_needed: &HashMap<RedeemerPointer, ScriptHash>,
    scripts_provided: &HashMap<ScriptHash, ScriptLang>,
    lookup_reference_script: &dyn Fn(&ScriptHash) -> Option<ReferenceScript>,
) -> Result<(), Phase2ValidationError> {
    let scripts_table = build_scripts_table(tx_deltas, utxos, lookup_reference_script);

    let tx_info = TxInfo::new(tx_deltas, utxos, genesis_values)?;

    let script_contexts = build_script_contexts(&tx_info, scripts_needed, scripts_provided)?;

    execute_scripts(
        &script_contexts,
        &scripts_table,
        cost_models,
        tx_deltas.is_valid,
    )
}
