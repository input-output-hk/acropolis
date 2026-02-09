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
//!
//! # Example
//!
//! ```ignore
//! use acropolis_module_tx_unpacker::validations::phase2::{
//!     evaluate_script, ExBudget, PlutusVersion,
//! };
//!
//! let budget = ExBudget::new(10_000_000_000, 10_000_000);
//! let cost_model: &[i64] = &[/* V3 cost model params */];
//!
//! let result = evaluate_script(
//!     &script_bytes,
//!     PlutusVersion::V3,
//!     None,           // datum (None for minting policies)
//!     &redeemer,      // CBOR-encoded PlutusData
//!     &script_context, // CBOR-encoded PlutusData
//!     cost_model,
//!     budget,
//! );
//! ```

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use acropolis_common::{DatumHash, PolicyId, ScriptHash, StakeAddress, UTxOIdentifier, Voter};
use rayon::prelude::*;
use rayon::ThreadPool;
use thiserror::Error;
use uplc_turbo::{
    arena::Arena, binder::DeBruijn, data::PlutusData, flat, machine::MachineError,
    program::Program, term::Term,
};

// Re-export PlutusVersion for use in tests and by consumers
pub use uplc_turbo::machine::PlutusVersion;

// =============================================================================
// Evaluator Thread Pool
// =============================================================================
// Real Plutus scripts (4-12KB) can have deep AST structures that cause stack
// overflow with the default 2MB stack. We use a dedicated thread pool with
// larger stacks (16MB) for script evaluation.

/// Stack size for evaluator threads: 16MB
/// This is sufficient for real mainnet scripts up to ~13KB.
const EVALUATOR_STACK_SIZE: usize = 16 * 1024 * 1024;

/// Number of threads in the evaluator pool (matches CPU cores by default)
fn evaluator_thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// Global thread pool with large stacks for script evaluation.
///
/// This pool is lazily initialized on first use and shared across all
/// script evaluations. Each thread has a 16MB stack to handle deep
/// recursion in the uplc-turbo evaluator.
static EVALUATOR_POOL: OnceLock<ThreadPool> = OnceLock::new();

/// Get (or create) the evaluator thread pool.
fn evaluator_pool() -> &'static ThreadPool {
    EVALUATOR_POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(evaluator_thread_count())
            .stack_size(EVALUATOR_STACK_SIZE)
            .thread_name(|i| format!("plutus-eval-{}", i))
            .build()
            .expect("Failed to create evaluator thread pool")
    })
}

// =============================================================================
// T006: ExBudget struct
// =============================================================================

/// Execution budget tracking for Plutus script evaluation.
///
/// Tracks CPU steps and memory units consumed during script execution.
/// Used to verify scripts don't exceed their allocated budgets.
///
/// # Fields
///
/// * `cpu` - CPU steps consumed or available
/// * `mem` - Memory units consumed or available
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExBudget {
    /// CPU steps consumed
    pub cpu: i64,
    /// Memory units consumed
    pub mem: i64,
}

impl ExBudget {
    /// Create a new execution budget with the given CPU and memory limits.
    ///
    /// # Arguments
    ///
    /// * `cpu` - Maximum CPU steps allowed
    /// * `mem` - Maximum memory units allowed
    pub fn new(cpu: i64, mem: i64) -> Self {
        Self { cpu, mem }
    }
}

impl From<ExBudget> for uplc_turbo::machine::ExBudget {
    fn from(budget: ExBudget) -> Self {
        uplc_turbo::machine::ExBudget {
            cpu: budget.cpu,
            mem: budget.mem,
        }
    }
}

impl From<uplc_turbo::machine::ExBudget> for ExBudget {
    fn from(budget: uplc_turbo::machine::ExBudget) -> Self {
        Self {
            cpu: budget.cpu,
            mem: budget.mem,
        }
    }
}

// =============================================================================
// EvalResult: Script evaluation result with timing
// =============================================================================

/// Result of a successful script evaluation including timing metrics.
///
/// This struct captures both the execution budget consumed and the wall-clock
/// time taken for evaluation, enabling performance monitoring and SC-001
/// compliance verification (<0.1s per script at p95).
#[derive(Debug, Clone, Copy)]
pub struct EvalResult {
    /// Execution budget consumed by the script
    pub consumed_budget: ExBudget,
    /// Wall-clock time taken for evaluation
    pub elapsed: Duration,
}

impl EvalResult {
    /// Create a new evaluation result.
    pub fn new(consumed_budget: ExBudget, elapsed: Duration) -> Self {
        Self {
            consumed_budget,
            elapsed,
        }
    }

    /// Check if the evaluation completed within the performance target.
    ///
    /// Per SC-001: script evaluation should complete in under 0.1 seconds.
    pub fn within_target(&self) -> bool {
        self.elapsed < Duration::from_millis(100)
    }

    /// Get elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> f64 {
        self.elapsed.as_secs_f64() * 1000.0
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

// =============================================================================
// T012: evaluate_script function
// =============================================================================

/// Evaluate a single Plutus script.
///
/// This is the core function for Phase 2 validation. It decodes a FLAT-encoded
/// Plutus script, applies the required arguments (datum, redeemer, script context),
/// and evaluates it within the given execution budget.
///
/// # Arguments
///
/// * `script_bytes` - FLAT-encoded Plutus script bytecode
/// * `plutus_version` - Which Plutus version (V1, V2, V3) the script uses
/// * `datum` - Optional CBOR-encoded PlutusData for spending scripts
/// * `redeemer` - CBOR-encoded PlutusData redeemer
/// * `script_context` - CBOR-encoded PlutusData script context
/// * `cost_model` - Cost model parameters from protocol parameters
/// * `budget` - Maximum CPU and memory budget for execution
///
/// # Returns
///
/// * `Ok(ExBudget)` - Consumed budget on successful script execution
/// * `Err(Phase2Error)` - Error if script fails or exceeds budget
///
/// # Script Arguments
///
/// Spending validators receive 3 arguments: `datum`, `redeemer`, `script_context`
/// Minting policies receive 2 arguments: `redeemer`, `script_context`
/// Other script purposes also receive 2 arguments (no datum).
///
/// # Example
///
/// ```ignore
/// let result = evaluate_script(
///     &script_flat_bytes,
///     PlutusVersion::V3,
///     Some(&datum_cbor),  // For spending validators
///     &redeemer_cbor,
///     &context_cbor,
///     &cost_model_params,
///     ExBudget::new(10_000_000_000, 10_000_000),
/// );
/// ```
pub fn evaluate_script(
    script_bytes: &[u8],
    plutus_version: PlutusVersion,
    datum: Option<&[u8]>,
    redeemer: &[u8],
    script_context: &[u8],
    cost_model: &[i64],
    budget: ExBudget,
) -> Result<EvalResult, Phase2Error> {
    // Copy inputs for thread-safe evaluation
    let script_bytes = script_bytes.to_vec();
    let datum = datum.map(|d| d.to_vec());
    let redeemer = redeemer.to_vec();
    let script_context = script_context.to_vec();
    let cost_model = cost_model.to_vec();

    // Run evaluation on the dedicated thread pool with larger stack
    evaluator_pool().install(|| {
        evaluate_script_inner(
            &script_bytes,
            plutus_version,
            datum.as_deref(),
            &redeemer,
            &script_context,
            &cost_model,
            budget,
        )
    })
}

/// Inner evaluation function that runs on the evaluator thread pool.
///
/// This is separated from the public API to allow the recursive evaluation
/// to run on threads with larger stacks (16MB vs default 2MB).
fn evaluate_script_inner(
    script_bytes: &[u8],
    plutus_version: PlutusVersion,
    datum: Option<&[u8]>,
    redeemer: &[u8],
    script_context: &[u8],
    cost_model: &[i64],
    budget: ExBudget,
) -> Result<EvalResult, Phase2Error> {
    // Start timing
    let start = Instant::now();

    // Create arena for memory allocation (1MB capacity)
    let arena = Arena::new();

    // Decode the FLAT-encoded script
    let program: &Program<DeBruijn> = flat::decode(&arena, script_bytes)
        .map_err(|e| Phase2Error::DecodeFailed(ScriptHash::default(), e.to_string()))?;

    // Decode redeemer from CBOR to PlutusData
    let redeemer_data = PlutusData::from_cbor(&arena, redeemer).map_err(|e| {
        Phase2Error::DecodeFailed(
            ScriptHash::default(),
            format!("Failed to decode redeemer: {}", e),
        )
    })?;

    // Decode script context from CBOR to PlutusData
    let context_data = PlutusData::from_cbor(&arena, script_context).map_err(|e| {
        Phase2Error::DecodeFailed(
            ScriptHash::default(),
            format!("Failed to decode script context: {}", e),
        )
    })?;

    // Apply arguments to the script based on presence of datum
    let applied = if let Some(datum_bytes) = datum {
        // Spending validator: apply datum, redeemer, context (3 args)
        let datum_data = PlutusData::from_cbor(&arena, datum_bytes).map_err(|e| {
            Phase2Error::DecodeFailed(
                ScriptHash::default(),
                format!("Failed to decode datum: {}", e),
            )
        })?;

        program
            .apply(&arena, Term::data(&arena, datum_data))
            .apply(&arena, Term::data(&arena, redeemer_data))
            .apply(&arena, Term::data(&arena, context_data))
    } else {
        // Minting policy or other: apply redeemer, context (2 args)
        program
            .apply(&arena, Term::data(&arena, redeemer_data))
            .apply(&arena, Term::data(&arena, context_data))
    };

    // Evaluate the script with cost model and budget
    let result = applied.eval_with_params(&arena, plutus_version, cost_model, budget.into());

    // Handle the evaluation result
    match result.term {
        Ok(_) => {
            // Script succeeded - return consumed budget and timing
            let elapsed = start.elapsed();
            Ok(EvalResult::new(result.info.consumed_budget.into(), elapsed))
        }
        Err(MachineError::ExplicitErrorTerm) => {
            // Script explicitly failed via `error` builtin
            Err(Phase2Error::ScriptFailed(
                ScriptHash::default(),
                "Script called error".to_string(),
            ))
        }
        Err(MachineError::OutOfExError(remaining)) => {
            // Script exceeded execution budget
            let consumed = ExBudget::new(budget.cpu - remaining.cpu, budget.mem - remaining.mem);
            Err(Phase2Error::BudgetExceeded(
                ScriptHash::default(),
                consumed.cpu,
                consumed.mem,
            ))
        }
        Err(e) => {
            // Other evaluation error
            Err(Phase2Error::ScriptFailed(
                ScriptHash::default(),
                format!("{}", e),
            ))
        }
    }
}

// =============================================================================
// Raw Program Evaluation (for benchmarks and pre-applied scripts)
// =============================================================================

/// Result of raw program evaluation.
///
/// Similar to `EvalResult` but without budget tracking (raw evaluation
/// doesn't use cost model budgets).
#[derive(Debug, Clone)]
pub struct RawEvalResult {
    /// Wall-clock time taken for evaluation
    pub elapsed: Duration,
}

impl RawEvalResult {
    /// Create a new raw evaluation result.
    pub fn new(elapsed: Duration) -> Self {
        Self { elapsed }
    }

    /// Check if the evaluation completed within the performance target.
    ///
    /// Per SC-001: script evaluation should complete in under 0.1 seconds.
    pub fn within_target(&self) -> bool {
        self.elapsed < Duration::from_millis(100)
    }

    /// Get elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> f64 {
        self.elapsed.as_secs_f64() * 1000.0
    }
}

/// Evaluate a raw FLAT-encoded Plutus program without argument application.
///
/// This function is intended for benchmark testing with pre-applied programs
/// (like those from the uplc-turbo benchmark suite). It uses the same evaluator
/// thread pool with large stacks as the main `evaluate_script()` function.
///
/// # Arguments
///
/// * `flat_bytes` - FLAT-encoded Plutus program bytecode
///
/// # Returns
///
/// * `Ok(RawEvalResult)` - Evaluation timing on success
/// * `Err(String)` - Error message on failure
///
/// # Example
///
/// ```ignore
/// let script_bytes = std::fs::read("benchmark.flat")?;
/// let result = evaluate_raw_flat_program(&script_bytes)?;
/// println!("Evaluated in {:.3}ms", result.elapsed_ms());
/// ```
pub fn evaluate_raw_flat_program(flat_bytes: &[u8]) -> Result<RawEvalResult, String> {
    let flat_bytes = flat_bytes.to_vec();

    // Run evaluation on the dedicated thread pool with larger stack
    evaluator_pool().install(|| evaluate_raw_flat_program_inner(&flat_bytes))
}

/// Inner evaluation function for raw FLAT programs.
///
/// Runs on the evaluator thread pool with 16MB stacks.
fn evaluate_raw_flat_program_inner(flat_bytes: &[u8]) -> Result<RawEvalResult, String> {
    let arena = Arena::new();

    // Decode the FLAT-encoded program
    let program: &Program<DeBruijn> =
        flat::decode(&arena, flat_bytes).map_err(|e| format!("Decode failed: {:?}", e))?;

    // Evaluate the program directly (no argument application)
    let start = Instant::now();
    let result = program.eval(&arena);
    let elapsed = start.elapsed();

    // Check if evaluation succeeded
    result.term.map_err(|e| format!("Evaluation failed: {:?}", e))?;

    Ok(RawEvalResult::new(elapsed))
}

// =============================================================================
// T021: build_script_context helper
// =============================================================================

/// Build a ScriptContext as PlutusData for script evaluation.
///
/// The ScriptContext structure varies by Plutus version:
/// - V1/V2: ScriptContext = Constr 0 [TxInfo, ScriptPurpose]
/// - V3: ScriptContext = Constr 0 [TxInfo, Redeemer, ScriptInfo]
///
/// # Arguments
///
/// * `arena` - Arena allocator for PlutusData construction
/// * `tx_info` - Pre-built TxInfo as PlutusData
/// * `purpose` - The script purpose identifying why the script runs
/// * `plutus_version` - Target Plutus version for context structure
///
/// # Returns
///
/// Reference to arena-allocated ScriptContext PlutusData
///
/// # Note
///
/// This is a simplified implementation. A full implementation would need to
/// properly serialize the purpose variants according to the Plutus specification.
#[allow(dead_code)]
pub fn build_script_context<'a>(
    arena: &'a Arena,
    tx_info: &'a PlutusData<'a>,
    _purpose: &ScriptPurpose,
    _plutus_version: PlutusVersion,
) -> &'a PlutusData<'a> {
    // For now, return a minimal ScriptContext structure
    // Full implementation would build the correct ScriptPurpose variant

    // ScriptContext = Constr 0 [TxInfo, ScriptPurpose]
    // Using empty constr for purpose placeholder
    let purpose_data = PlutusData::constr(arena, 0, &[]);

    // Allocate the context fields array in the arena
    let context_fields: &mut [&PlutusData; 2] = arena.alloc([tx_info, purpose_data]);
    PlutusData::constr(arena, 0, context_fields.as_slice())
}

// =============================================================================
// T026-T027: validate_transaction_phase2 - Extract scripts and match with redeemers
// =============================================================================

/// Input required to validate a single script execution.
#[derive(Debug)]
pub struct ScriptInput<'a> {
    /// The script hash identifying this script
    pub script_hash: ScriptHash,
    /// FLAT-encoded Plutus script bytecode
    pub script_bytes: &'a [u8],
    /// Plutus version (V1, V2, V3)
    pub plutus_version: PlutusVersion,
    /// The purpose of this script execution
    pub purpose: ScriptPurpose,
    /// Optional datum (for spending validators)
    pub datum: Option<&'a [u8]>,
    /// CBOR-encoded redeemer data
    pub redeemer: &'a [u8],
    /// Execution units allocated for this script
    pub ex_units: ExBudget,
}

/// Result of validating a transaction's Phase 2 scripts.
#[derive(Debug)]
pub struct Phase2ValidationResult {
    /// Total budget consumed by all scripts
    pub total_consumed: ExBudget,
    /// Total wall-clock time for all script evaluations
    pub total_elapsed: Duration,
    /// Individual script results (script_hash -> consumed budget, elapsed time)
    pub script_results: Vec<(ScriptHash, EvalResult)>,
}

/// Validate all Plutus scripts in a transaction.
///
/// This function orchestrates Phase 2 validation by:
/// 1. Extracting all scripts that need evaluation from the transaction
/// 2. Matching each script with its corresponding redeemer
/// 3. Evaluating each script with the appropriate arguments
/// 4. Collecting and reporting results
///
/// # Arguments
///
/// * `scripts` - Collection of scripts to validate with their inputs
/// * `cost_model_v1` - Cost model parameters for Plutus V1 scripts
/// * `cost_model_v2` - Cost model parameters for Plutus V2 scripts  
/// * `cost_model_v3` - Cost model parameters for Plutus V3 scripts
/// * `script_context` - CBOR-encoded ScriptContext for all scripts
///
/// # Returns
///
/// * `Ok(Phase2ValidationResult)` - All scripts executed successfully
/// * `Err(Phase2Error)` - First script failure encountered
///
/// # Note
///
/// Executes scripts in parallel using a dedicated thread pool with larger stacks
/// (16MB) to handle deep recursion in the uplc-turbo evaluator. Each script gets
/// its own arena allocator for thread safety (FR-009: constant memory per script).
pub fn validate_transaction_phase2(
    scripts: &[ScriptInput<'_>],
    cost_model_v1: &[i64],
    cost_model_v2: &[i64],
    cost_model_v3: &[i64],
    script_context: &[u8],
) -> Result<Phase2ValidationResult, Phase2Error> {
    // Start timing the overall parallel execution
    let overall_start = Instant::now();

    // Prepare owned copies of script data for thread-safe parallel execution
    let script_data: Vec<_> = scripts
        .iter()
        .map(|s| {
            (
                s.script_hash,
                s.script_bytes.to_vec(),
                s.plutus_version,
                s.datum.map(|d| d.to_vec()),
                s.redeemer.to_vec(),
                s.ex_units,
            )
        })
        .collect();

    let cost_model_v1 = cost_model_v1.to_vec();
    let cost_model_v2 = cost_model_v2.to_vec();
    let cost_model_v3 = cost_model_v3.to_vec();
    let script_context = script_context.to_vec();

    // Execute all scripts in parallel on the evaluator thread pool
    // This pool has 16MB stacks to handle large mainnet scripts
    let results: Vec<Result<(ScriptHash, EvalResult), Phase2Error>> = evaluator_pool().install(|| {
        script_data
            .par_iter()
            .map(|(script_hash, script_bytes, plutus_version, datum, redeemer, ex_units)| {
                // Select appropriate cost model based on Plutus version
                let cost_model = match plutus_version {
                    PlutusVersion::V1 => &cost_model_v1,
                    PlutusVersion::V2 => &cost_model_v2,
                    PlutusVersion::V3 => &cost_model_v3,
                };

                // Evaluate the script directly (we're already on the large-stack pool)
                evaluate_script_inner(
                    script_bytes,
                    *plutus_version,
                    datum.as_deref(),
                    redeemer,
                    &script_context,
                    cost_model,
                    *ex_units,
                )
                .map(|eval_result| (*script_hash, eval_result))
                .map_err(|e| {
                    // Enrich error with correct script hash
                    match e {
                        Phase2Error::ScriptFailed(_, msg) => {
                            Phase2Error::ScriptFailed(*script_hash, msg)
                        }
                        Phase2Error::BudgetExceeded(_, cpu, mem) => {
                            Phase2Error::BudgetExceeded(*script_hash, cpu, mem)
                        }
                        Phase2Error::DecodeFailed(_, msg) => {
                            Phase2Error::DecodeFailed(*script_hash, msg)
                        }
                        other => other,
                    }
                })
            })
            .collect()
    });

    // Total wall-clock time for the parallel execution
    let total_elapsed = overall_start.elapsed();

    // Check for any failures and collect successful results
    let mut total_consumed = ExBudget::default();
    let mut script_results = Vec::with_capacity(scripts.len());

    for result in results {
        let (script_hash, eval_result) = result?;
        total_consumed.cpu += eval_result.consumed_budget.cpu;
        total_consumed.mem += eval_result.consumed_budget.mem;
        script_results.push((script_hash, eval_result));
    }

    Ok(Phase2ValidationResult {
        total_consumed,
        total_elapsed,
        script_results,
    })
}

/// Convert from acropolis_common ScriptType to uplc PlutusVersion.
///
/// Returns None for native scripts (which don't need Phase 2 validation).
pub fn script_type_to_plutus_version(
    script_type: &acropolis_common::ScriptType,
) -> Option<PlutusVersion> {
    match script_type {
        acropolis_common::ScriptType::PlutusV1 => Some(PlutusVersion::V1),
        acropolis_common::ScriptType::PlutusV2 => Some(PlutusVersion::V2),
        acropolis_common::ScriptType::PlutusV3 => Some(PlutusVersion::V3),
        acropolis_common::ScriptType::Native => None,
    }
}

/// Convert from Phase2Error to common::Phase2ValidationError for integration.
impl From<Phase2Error> for acropolis_common::validation::Phase2ValidationError {
    fn from(err: Phase2Error) -> Self {
        match err {
            Phase2Error::ScriptFailed(script_hash, message) => {
                acropolis_common::validation::Phase2ValidationError::ScriptFailed {
                    script_hash,
                    message,
                }
            }
            Phase2Error::BudgetExceeded(script_hash, cpu, mem) => {
                acropolis_common::validation::Phase2ValidationError::BudgetExceeded {
                    script_hash,
                    cpu,
                    mem,
                }
            }
            Phase2Error::DecodeFailed(script_hash, reason) => {
                acropolis_common::validation::Phase2ValidationError::DecodeFailed {
                    script_hash,
                    reason,
                }
            }
            Phase2Error::MissingScript(index) => {
                acropolis_common::validation::Phase2ValidationError::MissingScript { index }
            }
            Phase2Error::MissingDatum(datum_hash) => {
                acropolis_common::validation::Phase2ValidationError::MissingDatum { datum_hash }
            }
            Phase2Error::MissingRedeemer(script_hash) => {
                acropolis_common::validation::Phase2ValidationError::MissingRedeemer { script_hash }
            }
        }
    }
}
