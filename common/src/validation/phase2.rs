//! Phase 2 (Plutus script execution) validation errors.
//!
//! These errors occur during script evaluation after Phase 1 validation passes.

use crate::{
    DatumHash, ExUnits, PlutusVersion, RedeemerPointer, RedeemerTag, ScriptHash, UTxOIdentifier,
};
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

/// Per-script outcome of phase-2 Plutus validation.
///
/// One `ScriptEvaluationOutcome` is produced for each Plutus script context that the
/// evaluator processed for a transaction. Native scripts are not represented (they do
/// not undergo phase-2 evaluation). The execution units are taken from the redeemer
/// in the transaction (i.e. the budget the transaction declared for the script).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptEvaluationOutcome {
    /// Hash of the script that was evaluated.
    pub script_hash: ScriptHash,

    /// Why this script ran (spend / mint / cert / reward / vote / propose).
    pub purpose: RedeemerTag,

    /// Plutus language version under which the script was evaluated.
    pub plutus_version: PlutusVersion,

    /// Execution units (memory, cpu/steps) declared by the redeemer for this script.
    pub ex_units: ExUnits,

    /// `true` iff the script evaluated successfully under phase-2 rules.
    pub is_success: bool,

    /// `None` on success. On failure, a short rendered error message (≤ 512 chars).
    pub error_message: Option<String>,
}

impl ScriptEvaluationOutcome {
    /// Maximum length of `error_message`. Longer messages are truncated at construction.
    pub const MAX_ERROR_MESSAGE_LEN: usize = 512;

    /// Construct an outcome, truncating an over-long error message to
    /// [`Self::MAX_ERROR_MESSAGE_LEN`] bytes.
    pub fn new(
        script_hash: ScriptHash,
        purpose: RedeemerTag,
        plutus_version: PlutusVersion,
        ex_units: ExUnits,
        is_success: bool,
        error_message: Option<String>,
    ) -> Self {
        let error_message = error_message.map(|mut m| {
            if m.len() > Self::MAX_ERROR_MESSAGE_LEN {
                m.truncate(Self::MAX_ERROR_MESSAGE_LEN);
            }
            m
        });
        Self {
            script_hash,
            purpose,
            plutus_version,
            ex_units,
            is_success,
            error_message,
        }
    }
}
