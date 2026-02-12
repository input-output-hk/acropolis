# Phase 2 Validation API Contract (Simplified)

**Feature**: 568-plutus-phase2-validation  
**Date**: 2026-02-06  
**Type**: Internal Rust module API

## Overview

This defines the minimal public API for Phase 2 validation. The design philosophy is: **two functions + one error type**.

---

## Public Functions

### `evaluate_script`

Core function that evaluates a single Plutus script.

```rust
/// Evaluate a single Plutus script
pub fn evaluate_script(
    script_bytes: &[u8],           // FLAT-encoded script bytecode
    plutus_version: PlutusVersion, // V1, V2, or V3
    datum: Option<&[u8]>,          // CBOR PlutusData (spending scripts only)
    redeemer: &[u8],               // CBOR PlutusData
    script_context: &[u8],         // CBOR PlutusData
    cost_model: &[i64],            // From protocol parameters
    budget: ExBudget,              // CPU/mem limit
) -> Result<ExBudget, Phase2Error>;
```

**Returns**: Consumed budget on success, error on failure.

---

### `validate_transaction_phase2`

Transaction-level function that validates all scripts in a transaction.

```rust
/// Validate all scripts in a transaction (Phase 2)
pub fn validate_transaction_phase2(
    tx: &MultiEraTx,                // Decoded transaction
    resolved_inputs: &[ResolvedInput], // UTxOs being spent
    cost_models: &CostModels,       // Protocol param cost models
    max_budget: ExBudget,           // Per-transaction limit
) -> Result<(), Phase2Error>;
```

**Behavior**:
1. Extracts scripts from witness set
2. Matches scripts with redeemers
3. Builds ScriptContext for each
4. Evaluates in parallel using `rayon::par_iter()`
5. Returns first error or `Ok(())`

---

## Types

### `ExBudget`

```rust
#[derive(Debug, Clone, Copy)]
pub struct ExBudget {
    pub cpu: i64,
    pub mem: i64,
}
```

### `Phase2Error`

```rust
#[derive(Debug, thiserror::Error)]
pub enum Phase2Error {
    #[error("Script {0} failed: {1}")]
    ScriptFailed(ScriptHash, String),
    
    #[error("Script {0} exceeded budget")]
    BudgetExceeded(ScriptHash),
    
    #[error("Script {0} decode failed: {1}")]
    DecodeFailed(ScriptHash, String),
    
    #[error("Missing script for redeemer at index {0}")]
    MissingScript(u32),
    
    #[error("Missing datum {0}")]
    MissingDatum(DatumHash),
    
    #[error("Missing redeemer for script {0}")]
    MissingRedeemer(ScriptHash),
}
```

### `ResolvedInput`

Input UTxO with resolved output data (needed for ScriptContext).

```rust
pub struct ResolvedInput {
    pub tx_in: TransactionInput,
    pub output: TransactionOutput,
}
```

---

## Integration Point

In `state.rs::validate()`:

```rust
// After Phase 1 validation passes
if config.phase2_enabled {
    phase2::validate_transaction_phase2(
        &tx,
        &resolved_inputs,
        &self.cost_models,
        self.max_budget,
    )?;
}
```

---

## File Structure

```
modules/tx_unpacker/src/validations/
├── mod.rs           # Add: pub mod phase2;
└── phase2.rs        # All Phase 2 code in single file
```

---

## Parallel Evaluation

```rust
use rayon::prelude::*;

let results: Result<Vec<ExBudget>, Phase2Error> = scripts
    .par_iter()
    .map(|s| evaluate_script(s.bytes, s.version, ...))
    .collect();
```

Each `evaluate_script` call is independent and thread-safe.
```
