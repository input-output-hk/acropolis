//! Plutus Phase 2 script validation.
//!
//! This module provides Phase 2 (script execution) validation for Plutus smart contracts
//! using the `uplc-turbo` crate from pragma-org/uplc.
//!
//! # Overview
//!
//! Phase 2 validation evaluates Plutus scripts after Phase 1 validation has passed.
//! It verifies that all scripts in a transaction execute successfully within their
//! allocated execution budgets.
//!
//! # Feature Flag
//!
//! Phase 2 validation is disabled by default. Enable it via configuration:
//! ```toml
//! [module.tx-unpacker]
//! phase2_enabled = true
//! ```

use acropolis_common::{DatumHash, PolicyId, ScriptHash, StakeAddress, UTxOIdentifier, Voter};
use thiserror::Error;

// =============================================================================
// T006: ExBudget struct
// =============================================================================

/// Execution budget tracking for Plutus script evaluation.
///
/// Tracks CPU steps and memory units consumed during script execution.
/// Used to verify scripts don't exceed their allocated budgets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExBudget {
    /// CPU steps consumed
    pub cpu: i64,
    /// Memory units consumed
    pub mem: i64,
}

impl ExBudget {
    /// Create a new execution budget with the given CPU and memory limits.
    pub fn new(cpu: i64, mem: i64) -> Self {
        Self { cpu, mem }
    }
}

// =============================================================================
// T007: Phase2Error enum
// =============================================================================

/// Error type for Phase 2 script validation failures.
///
/// All Phase 2 validation errors are captured in this enum, making error
/// handling and reporting consistent across the validation pipeline.
#[derive(Debug, Clone, Error, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Phase2Error {
    /// Script explicitly called the `error` builtin
    #[error("Script {0} failed: {1}")]
    ScriptFailed(ScriptHash, String),

    /// Script exceeded CPU or memory budget
    #[error("Script {0} exceeded budget (cpu: {1}, mem: {2})")]
    BudgetExceeded(ScriptHash, i64, i64),

    /// Could not decode FLAT bytecode
    #[error("Script {0} decode failed: {1}")]
    DecodeFailed(ScriptHash, String),

    /// Missing script referenced by redeemer
    #[error("Missing script for redeemer at index {0}")]
    MissingScript(u32),

    /// Missing datum for spending input
    #[error("Missing datum {0}")]
    MissingDatum(DatumHash),

    /// Missing redeemer for script
    #[error("Missing redeemer for script {0}")]
    MissingRedeemer(ScriptHash),
}

// =============================================================================
// T008: ScriptPurpose enum
// =============================================================================

/// Identifies why a script is being evaluated.
///
/// This is used to build the correct `ScriptContext` for Plutus script evaluation.
/// Different purposes require different context structures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptPurpose {
    /// Spending a UTxO locked by a script
    Spending(UTxOIdentifier),

    /// Minting or burning tokens under a policy
    Minting(PolicyId),

    /// Publishing a certificate (stake delegation, pool registration, etc.)
    Certifying {
        /// Index of the certificate in the transaction
        index: u32,
    },

    /// Withdrawing rewards from a stake address
    Rewarding(StakeAddress),

    /// Voting on a governance action (Plutus V3 only)
    Voting(Voter),

    /// Proposing a governance action (Plutus V3 only)
    Proposing {
        /// Index of the proposal in the transaction
        index: u32,
    },
}

// TODO: T012 - Implement evaluate_script()
// TODO: T026 - Implement validate_transaction_phase2()
