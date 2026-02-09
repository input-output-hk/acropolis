# Implementation Plan: Plutus Phase 2 Script Validation

**Branch**: `568-plutus-phase2-validation` | **Date**: 2026-02-06 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/568-plutus-phase2-validation/spec.md`

## Summary

Integrate the pragma-org/uplc Plutus script evaluator into Acropolis to provide Phase 2 validation for blocks containing smart contract transactions. The integration will occur in the `tx_unpacker` module after existing Phase 1 validation, using the `uplc-turbo` crate's arena-based execution model for efficient, constant-memory script evaluation.

## Technical Context

**Language/Version**: Rust 2024 Edition  
**Primary Dependencies**: `uplc-turbo` (pragma-org/uplc), pallas, tokio  
**Storage**: N/A (stateless validation)  
**Testing**: cargo test, integration tests with fixture blocks  
**Target Platform**: Linux server (amd64/arm64)  
**Project Type**: Single module integration into existing monorepo  
**Performance Goals**: <0.1s per script evaluation, parallel multi-script execution  
**Constraints**: Constant memory usage across script evaluations, no modifications to uplc crate  
**Scale/Scope**: Handle blocks with 10+ scripts, mainnet-compatible validation

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Gate | Status | Notes |
|------|--------|-------|
| Rust 2024 Edition | ✅ PASS | Project uses Rust 2024 Edition |
| Tokio async runtime | ✅ PASS | Integration uses existing Tokio runtime |
| thiserror/anyhow errors | ✅ PASS | New error types use thiserror |
| Serde/CBOR serialization | ✅ PASS | Script bytecode is CBOR, using existing codec |
| Modular architecture | ✅ PASS | Integration in existing tx_unpacker module |
| No unwrap() | ✅ PASS | All error paths use Result with ? |
| Doc comments required | ✅ PASS | All public API will be documented |
| TDD workflow | ✅ PASS | Will write tests first for evaluator wrapper |
| Integration tests | ✅ PASS | Fixture-based block tests for CI |

**No violations - all gates pass.**

## Project Structure

### Documentation (this feature)

```text
specs/568-plutus-phase2-validation/
├── plan.md              # This file
├── research.md          # Codebase analysis + uplc API research
├── data-model.md        # Phase 2 validation types
├── quickstart.md        # Integration guide
├── contracts/           # Internal API contracts
│   └── phase2-validation-api.md
└── tasks.md             # Implementation tasks (created by /speckit.tasks)
```

### Source Code (repository root)

```text
modules/tx_unpacker/
├── src/
│   ├── lib.rs                    # Module entry point
│   ├── state.rs                  # Integration point: validate() method
│   ├── validations/
│   │   ├── mod.rs               # Phase 1 validation entry
│   │   └── phase2.rs            # NEW: Phase 2 validation (single file)
│   └── ...
├── Cargo.toml                    # Add uplc-turbo dependency
└── tests/
    └── phase2_test.rs           # Integration tests

common/src/
└── validation.rs                 # Existing: add Phase2Error variant
```

**Structure Decision**: Single `phase2.rs` file in validations directory. No subdirectory needed - the uplc crate does the heavy lifting. Keep it simple.

## TDD Approach

### Workflow

Following the constitution's TDD requirement, implementation proceeds in this cycle:

1. **Write failing test** - Define expected behavior before implementation
2. **Run test, observe red** - Confirm the test fails for the right reason
3. **Write minimal code** - Implement just enough to pass the test
4. **Run test, observe green** - Confirm the implementation works
5. **Refactor** - Clean up while keeping tests green
6. **Repeat** - Next test case

### Test Data

We need only two categories of test data:

#### 1. Minimal Hand-Crafted Scripts (inline in tests)

```rust
// In phase2_test.rs - no external files needed
const ALWAYS_SUCCEEDS: &[u8] = &[/* FLAT bytes for (program 1.0.0 (con unit ())) */];
const ALWAYS_FAILS: &[u8] = &[/* FLAT bytes for (program 1.0.0 (error)) */];
```

Compile these once using `pluton` CLI and embed as byte arrays.

#### 2. One Real Mainnet Transaction

Use an existing Conway transaction fixture from `tests/data/conway/` that contains Plutus scripts. This validates end-to-end integration.

### Test Implementation Order (TDD)

**Phase 1: Core Evaluation (3 tests)**

| # | Test | Implementation |
|---|------|----------------|
| 1 | `test_eval_always_succeeds` | Basic `evaluate_script()` function |
| 2 | `test_eval_always_fails` | Handle `MachineError::ExplicitErrorTerm` |
| 3 | `test_eval_budget_exceeded` | Handle `MachineError::OutOfExError` |

**Phase 2: Argument Application (2 tests)**

| # | Test | Implementation |
|---|------|----------------|
| 4 | `test_eval_spending_validator` | Apply 3 args: datum, redeemer, context |
| 5 | `test_eval_minting_policy` | Apply 2 args: redeemer, context |

**Phase 3: Integration (3 tests)**

| # | Test | Implementation |
|---|------|----------------|
| 6 | `test_phase2_disabled_skips_scripts` | Config flag check in `state.rs` |
| 7 | `test_phase2_enabled_validates_scripts` | Wire into validation flow |
| 8 | `test_parallel_multi_script_block` | Use `rayon::par_iter()` for concurrency |

### Running Tests

```bash
cargo test -p acropolis_module_tx_unpacker phase2
```

## Complexity Tracking

> No violations - complexity tracking not required.

---

## Implementation Task Sequence

### Setup (do once)

1. Add `uplc-turbo` to workspace `Cargo.toml`:
   ```toml
   [workspace.dependencies]
   uplc-turbo = { git = "https://github.com/pragma-org/uplc", package = "uplc" }
   ```

2. Add dependency to `modules/tx_unpacker/Cargo.toml`:
   ```toml
   [dependencies]
   uplc-turbo = { workspace = true }
   rayon = "1.10"  # For parallel iteration
   ```

3. Compile test scripts to FLAT bytes (embed in test file as `const` arrays)

### Phase 1: Core Evaluation Function

**Goal**: Single function that evaluates a script and returns success/failure.

```rust
// validations/phase2.rs
pub fn evaluate_script(
    script_bytes: &[u8],
    plutus_version: PlutusVersion,
    datum: Option<&[u8]>,      // CBOR-encoded PlutusData
    redeemer: &[u8],           // CBOR-encoded PlutusData  
    script_context: &[u8],     // CBOR-encoded PlutusData
    cost_model: &[i64],
    budget: ExBudget,
) -> Result<ExBudget, Phase2Error>;  // Returns consumed budget on success
```

**Reference**: [research.md § Question 7](research.md#question-7-how-does-the-uplc-turbo-crate-api-work)

### Phase 2: Transaction Validation

**Goal**: Validate all scripts in a transaction.

```rust
pub fn validate_transaction_phase2(
    tx: &MultiEraTx,
    cost_models: &CostModels,
    max_budget: ExBudget,
) -> Result<(), Phase2Error>;
```

This function:
1. Extracts scripts from witness set
2. Matches scripts to redeemers
3. Builds script context for each
4. Calls `evaluate_script()` for each (in parallel with `rayon`)

**Reference**: [research.md § Question 3](research.md#question-3-where-should-phase-2-validation-be-integrated)

### Phase 3: Integration

**Goal**: Wire into `state.rs::validate()` with config flag.

```rust
// In state.rs::validate()
if self.config.phase2_enabled {
    if let Err(e) = phase2::validate_transaction_phase2(&tx, &cost_models, budget) {
        return Err(ValidationError::Phase2(e));
    }
}
```

**Reference**: [research.md § Question 6](research.md#question-6-how-to-handle-configuration-flag)

---

## Simplified Type Summary

Only these new types are needed:

```rust
/// Error from Phase 2 validation
#[derive(Debug, thiserror::Error)]
pub enum Phase2Error {
    #[error("Script {0} failed: {1}")]
    ScriptFailed(ScriptHash, String),
    
    #[error("Script {0} exceeded budget")]
    BudgetExceeded(ScriptHash),
    
    #[error("Could not decode script {0}: {1}")]
    DecodeFailed(ScriptHash, String),
    
    #[error("Missing script for redeemer")]
    MissingScript,
}

/// Execution budget (re-export from uplc or define simply)
#[derive(Debug, Clone, Copy)]
pub struct ExBudget {
    pub cpu: i64,
    pub mem: i64,
}
```

**Note**: We do NOT need separate `EvalRequest`, `EvalOutcome`, `ScriptEvaluation`, `ScriptError`, or `Phase2ValidationError` types. One error enum is sufficient.
