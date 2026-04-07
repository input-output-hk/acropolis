//! Phase 2 (Plutus script execution) validation errors.
//!
//! These errors occur during script evaluation after Phase 1 validation passes.

use crate::{DatumHash, PlutusVersion, RedeemerPointer, ScriptHash, UTxOIdentifier};
use thiserror::Error;

/// Phase 2 (Plutus script execution) validation errors.
///
/// These errors occur during script evaluation after Phase 1 validation passes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error, PartialEq, Eq)]
pub enum Phase2ValidationError {
    #[error("script context construction failed: {0}")]
    ScriptContextError(#[from] ScriptContextError),

    #[error("failed to flat-decode script: {0}")]
    FlatDecodingError(String),

    #[error("missing cost model for Plutus version: {0:?}")]
    MissingCostModel(PlutusVersion),

    #[error("missing script for hash: {0:?}")]
    MissingScriptForHash(ScriptHash),

    #[error("uplc machine error {0}")]
    UplcMachineError(#[from] UplcMachineError),

    #[error("expected scripts to fail but didn't (is_valid = false)")]
    ValidityStateError,
}

/// ScriptContextError occurs during script context construction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error, PartialEq, Eq)]
pub enum ScriptContextError {
    #[error("missing input UTxO: {0}")]
    MissingInput(UTxOIdentifier),

    #[error("missing script for redeemer: {0:?}")]
    MissingScript(RedeemerPointer),

    #[error("missing validation data: {0}")]
    MissingValidationData(String),

    #[error("CBOR decode failed: {0}")]
    CborDecodeFailed(String),

    #[error("unsupported address type: {0}")]
    UnsupportedAddress(String),

    #[error("unsupported certificate type for Plutus version")]
    UnsupportedCertificate,

    #[error("unsupported reference script for Plutus v1")]
    UnsupportedReferenceScript,

    #[error("Unsupported Script Purpose for Plutus version V1 or V2")]
    UnsupportedScriptPurpose,
}

/// UplcMachineError occur during executing Plutus scripts in Phase 2 validation. They include budget exceedance, and other script execution errors.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error, PartialEq, Eq)]
pub enum UplcMachineError {
    /// Script explicitly called the `error` builtin
    #[error("Script {script_hash} failed: {message}")]
    ScriptFailed {
        script_hash: ScriptHash,
        message: String,
    },

    /// Script exceeded CPU or memory budget
    #[error("Script {script_hash} exceeded budget (cpu: {cpu}, mem: {mem})")]
    BudgetExceeded {
        script_hash: ScriptHash,
        cpu: i64,
        mem: i64,
    },

    /// Could not decode FLAT bytecode
    #[error("Script {script_hash} decode failed: {reason}")]
    DecodeFailed {
        script_hash: ScriptHash,
        reason: String,
    },

    /// Missing script referenced by redeemer
    #[error("Missing script for redeemer at index {index}")]
    MissingScript { index: u32 },

    /// Missing datum for spending input
    #[error("Missing datum {datum_hash}")]
    MissingDatum { datum_hash: DatumHash },

    /// Missing redeemer for script
    #[error("Missing redeemer for script {script_hash}")]
    MissingRedeemer { script_hash: ScriptHash },
}
