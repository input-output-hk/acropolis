#![allow(dead_code)]

use acropolis_common::{validation::UTxOWValidationError, ScriptHash, ScriptType};
use std::collections::{HashMap, HashSet};

/// There are new Babbage era UTxOW rules
/// 1. MalformedScriptWitnesses
/// 2. MalformedReferenceScripts
pub fn validate(
    _scripts_provided: &HashMap<ScriptHash, ScriptType>,
    _script_witness_hashes: &HashSet<ScriptHash>,
) -> Result<(), Box<UTxOWValidationError>> {
    // TODO:
    // Check script witnesses and reference scripts
    // are correctly formed

    Ok(())
}
