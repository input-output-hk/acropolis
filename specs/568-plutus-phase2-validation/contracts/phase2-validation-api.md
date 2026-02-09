# Phase 2 Validation Internal API Contract

**Feature**: 568-plutus-phase2-validation  
**Date**: 2026-02-06  
**Type**: Internal Rust module API

## Overview

This contract defines the internal API for the Phase 2 validation subsystem within the `tx_unpacker` module. It is not a REST/HTTP API but rather the Rust function signatures and types that form the integration boundary.

---

## Module: `validations::phase2`

### Public Functions

#### `validate_transaction_scripts`

Main entry point for Phase 2 validation of a transaction.

```rust
/// Validate all Plutus scripts in a transaction
/// 
/// # Arguments
/// * `tx` - Decoded transaction with witnesses
/// * `utxo_resolver` - Resolver for looking up input UTxOs
/// * `protocol_params` - Current protocol parameters
/// * `config` - Phase 2 validation configuration
/// 
/// # Returns
/// * `Ok(Vec<ScriptEvaluation>)` - All scripts evaluated (may contain failures)
/// * `Err(Phase2ValidationError)` - Validation could not complete
pub fn validate_transaction_scripts<'a>(
    arena: &'a Arena,
    tx: &MaryTx,  // Or appropriate pallas tx type
    utxo_resolver: &impl UtxoResolver,
    protocol_params: &ProtocolParams,
    config: &Phase2Config,
) -> Result<Vec<ScriptEvaluation>, Phase2ValidationError>;
```

#### `evaluate_script`

Evaluate a single Plutus script.

```rust
/// Evaluate a single Plutus script with given arguments
/// 
/// # Arguments
/// * `arena` - Arena allocator for evaluation
/// * `script` - Script bytecode and version
/// * `datum` - Optional datum (required for spending scripts)
/// * `redeemer` - Redeemer data
/// * `context` - Script context with transaction info
/// * `budget` - Execution budget limit
/// * `cost_model` - Protocol parameter cost model
/// 
/// # Returns
/// * `ScriptEvaluation` containing success/failure and budget consumed
pub fn evaluate_script<'a>(
    arena: &'a Arena,
    script: &PlutusScript,
    datum: Option<&PlutusData>,
    redeemer: &PlutusData,
    context: &ScriptContext,
    budget: ExBudget,
    cost_model: &[i64],
) -> ScriptEvaluation;
```

---

### Configuration Types

#### `Phase2Config`

```rust
/// Configuration for Phase 2 validation behavior
#[derive(Debug, Clone, Default)]
pub struct Phase2Config {
    /// Enable Phase 2 validation (default: false)
    pub enabled: bool,
    
    /// Enable parallel script evaluation (default: true when enabled)
    pub parallel_evaluation: bool,
    
    /// Arena capacity for script evaluation in bytes (default: 1MB)
    pub arena_capacity: usize,
}

impl Phase2Config {
    pub const DEFAULT_ARENA_CAPACITY: usize = 1_024_000;
}
```

---

### Result Types

#### `ScriptEvaluation`

```rust
/// Result of evaluating a single script
#[derive(Debug, Clone)]
pub struct ScriptEvaluation {
    /// Script that was evaluated
    pub script_hash: ScriptHash,
    
    /// Purpose for which script was run
    pub purpose: ScriptPurpose,
    
    /// Evaluation outcome
    pub outcome: EvalOutcome,
}
```

#### `EvalOutcome`

```rust
/// Outcome of script evaluation
#[derive(Debug, Clone)]
pub enum EvalOutcome {
    /// Script evaluated successfully
    Success {
        /// Budget consumed by evaluation
        consumed_budget: ExBudget,
        /// Debug logs from script (if any)
        logs: Vec<String>,
    },
    
    /// Script evaluation failed
    Failure {
        /// Reason for failure
        error: ScriptError,
        /// Budget consumed before failure
        consumed_budget: ExBudget,
        /// Debug logs up to failure point
        logs: Vec<String>,
    },
}

impl EvalOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, EvalOutcome::Success { .. })
    }
    
    pub fn consumed_budget(&self) -> ExBudget {
        match self {
            EvalOutcome::Success { consumed_budget, .. } => *consumed_budget,
            EvalOutcome::Failure { consumed_budget, .. } => *consumed_budget,
        }
    }
}
```

---

### Error Types

#### `ScriptError`

```rust
/// Specific error from script evaluation
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScriptError {
    #[error("Script explicitly failed")]
    ExplicitError,
    
    #[error("Execution budget exceeded: cpu={cpu}, mem={mem}")]
    BudgetExceeded { cpu: i64, mem: i64 },
    
    #[error("Script deserialization failed: {reason}")]
    DeserializationFailed { reason: String },
    
    #[error("Missing datum for script")]
    MissingDatum,
    
    #[error("Machine error: {message}")]
    MachineError { message: String },
}
```

#### `Phase2ValidationError`

```rust
/// Errors that prevent Phase 2 validation from completing
#[derive(Debug, Clone, thiserror::Error)]
pub enum Phase2ValidationError {
    #[error("Missing script {script_hash} referenced by redeemer")]
    MissingScript { script_hash: ScriptHash },
    
    #[error("Missing redeemer for script purpose {purpose:?}")]
    MissingRedeemer { purpose: ScriptPurpose },
    
    #[error("Failed to resolve UTxO {tx_in:?}")]
    UtxoResolutionFailed { tx_in: TransactionInput },
    
    #[error("Cost model not available for Plutus {version:?}")]
    MissingCostModel { version: PlutusVersion },
    
    #[error("Failed to build script context: {reason}")]
    ContextBuildFailed { reason: String },
}
```

---

### Traits

#### `UtxoResolver`

```rust
/// Trait for resolving UTxO data needed by script context
pub trait UtxoResolver {
    /// Resolve a transaction input to its output data
    fn resolve(&self, tx_in: &TransactionInput) -> Option<ResolvedTxOut>;
}

/// Resolved transaction output with all data needed for scripts
pub struct ResolvedTxOut {
    pub address: Address,
    pub value: Value,
    pub datum: Option<DatumOption>,
    pub script_ref: Option<ScriptRef>,
}

pub enum DatumOption {
    Hash(DatumHash),
    Inline(PlutusData),
}
```

---

## Integration Contract

### Entry Point in `state.rs`

```rust
impl State {
    pub fn validate(
        &self,
        block_info: &BlockInfo,
        txs_msg: &RawTxsMessage,
        genesis_delegs: &GenesisDelegates,
        phase2_config: &Phase2Config,  // NEW parameter
    ) -> Result<(), Box<ValidationError>> {
        let mut bad_transactions = Vec::new();
        
        // Create arena once per block for efficiency
        let bump = Bump::with_capacity(phase2_config.arena_capacity);
        let arena = Arena::from_bump(bump);
        
        for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
            // Phase 1 validation (existing)
            if let Err(e) = self.validate_transaction(block_info, raw_tx, genesis_delegs) {
                bad_transactions.push((tx_index, *e));
                continue;  // Skip Phase 2 if Phase 1 fails
            }
            
            // Phase 2 validation (NEW)
            if phase2_config.enabled && tx_has_scripts(raw_tx) {
                let evals = validate_transaction_scripts(
                    &arena,
                    raw_tx,
                    &self.utxo_resolver,
                    &self.protocol_params,
                    phase2_config,
                )?;
                
                // Check for any failed scripts
                for eval in evals {
                    if let EvalOutcome::Failure { error, .. } = eval.outcome {
                        bad_transactions.push((
                            tx_index,
                            ValidationError::Phase2(Phase2ValidationError::ScriptFailed {
                                script_hash: eval.script_hash,
                                purpose: eval.purpose,
                                error,
                            })
                        ));
                    }
                }
                
                // Reset arena between transactions
                arena.reset();
            }
        }
        
        // ... rest of validation reporting
    }
}
```

---

## Module File Structure

```
modules/tx_unpacker/src/validations/
├── mod.rs                    # Existing: add `pub mod phase2;`
└── phase2/
    ├── mod.rs               # Re-exports, validate_transaction_scripts()
    ├── config.rs            # Phase2Config
    ├── evaluator.rs         # evaluate_script(), uplc integration
    ├── context.rs           # ScriptContext, TxInfo builders
    ├── error.rs             # ScriptError, Phase2ValidationError
    └── types.rs             # ScriptEvaluation, EvalOutcome, etc.
```

---

## Thread Safety Notes

1. **Arena is not Send/Sync** - each thread needs its own arena
2. For parallel evaluation, create arena per-thread in rayon scope
3. Protocol params and cost models are immutable references - safe to share

```rust
// Parallel evaluation pattern
use rayon::prelude::*;

let evaluations: Vec<ScriptEvaluation> = scripts
    .par_iter()
    .map(|script_req| {
        // Each thread gets its own arena
        let bump = Bump::with_capacity(config.arena_capacity);
        let arena = Arena::from_bump(bump);
        
        evaluate_script(&arena, script_req, ...)
    })
    .collect();
```
