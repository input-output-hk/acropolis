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

## TDD Approach

### Workflow

Following the constitution's TDD requirement, implementation proceeds in this cycle:

1. **Write failing test** - Define expected behavior before implementation
2. **Run test, observe red** - Confirm the test fails for the right reason
3. **Write minimal code** - Implement just enough to pass the test
4. **Run test, observe green** - Confirm the implementation works
5. **Refactor** - Clean up while keeping tests green
6. **Repeat** - Next test case

### Test Data Sources

#### Source 1: Plutus Conformance Tests (pragma-org/uplc)

The `uplc` crate includes a comprehensive conformance test suite from the official Plutus repository:

```bash
# Download official Plutus test cases into uplc repo
just download-plutus-tests
```

These are `.uplc` text format files with expected outputs. We can compile them to FLAT bytecode for our tests.

**Location**: `https://github.com/IntersectMBO/plutus/tree/master/plutus-conformance/test-cases/uplc/evaluation`

**Examples**:
- `example/fibonacci/fibonacci.uplc` - Compute fibonacci(15) = 610
- `example/DivideByZero/DivideByZero.uplc` - Triggers evaluation failure
- `builtin/semantics/addInteger/*` - Arithmetic operations

#### Source 2: Hand-Crafted Minimal Scripts

Create minimal scripts that test specific validation paths:

| Script | Behavior | Expected Result |
|--------|----------|-----------------|
| `always_succeeds.flat` | `(program 1.0.0 (con unit ()))` | ✅ Success |
| `always_fails.flat` | `(program 1.0.0 (error))` | ❌ ExplicitError |
| `add_one.flat` | `(lam x [(builtin addInteger) x (con integer 1)])` | ✅ Returns x+1 |
| `budget_hog.flat` | Deep recursion exceeding budget | ❌ BudgetExceeded |

#### Source 3: Mainnet Transaction Samples

Extract real scripts from mainnet transactions in the existing test fixtures:

```text
modules/tx_unpacker/tests/data/
├── conway/
│   └── <tx_hash>/
│       ├── context.json      # Slot, protocol params
│       ├── tx.cbor           # Full transaction
│       └── scripts/          # NEW: extracted scripts
│           ├── spend_0.flat
│           └── mint_0.flat
```

**Extraction process**:
1. Parse transaction CBOR with pallas
2. Extract witness set scripts
3. CBOR-unwrap to get FLAT bytes
4. Save with known validation result from mainnet

#### Source 4: Blockfrost API Queries

For additional real-world scripts:

```bash
# Get scripts from a specific transaction
curl -H "project_id: $BLOCKFROST_KEY" \
  "https://cardano-mainnet.blockfrost.io/api/v0/txs/<hash>/scripts"
```

### Test Fixture Structure

```text
modules/tx_unpacker/tests/data/phase2/
├── fixtures.json                    # Index of all test cases
├── minimal/
│   ├── always_succeeds/
│   │   ├── script.flat              # FLAT-encoded script bytes (hex)
│   │   ├── script.uplc              # Human-readable source
│   │   └── expected.json            # { "result": "success", "budget": {...} }
│   ├── always_fails/
│   │   └── ...
│   └── budget_exceeded/
│       └── ...
├── validators/                      # Scripts requiring arguments
│   ├── simple_spend/
│   │   ├── script.flat
│   │   ├── datum.cbor
│   │   ├── redeemer.cbor
│   │   ├── context.cbor
│   │   └── expected.json
│   └── ...
└── mainnet/                         # Real mainnet samples
    └── <tx_hash>/
        └── ...
```

### Test Case Format (fixtures.json)

```json
{
  "test_cases": [
    {
      "name": "always_succeeds",
      "description": "Trivial script that returns unit",
      "script_path": "minimal/always_succeeds/script.flat",
      "plutus_version": "V3",
      "arguments": null,
      "expected": {
        "result": "success",
        "consumed_budget": { "cpu": 100, "mem": 100 }
      }
    },
    {
      "name": "always_fails",
      "description": "Script that calls error builtin",
      "script_path": "minimal/always_fails/script.flat",
      "plutus_version": "V3",
      "arguments": null,
      "expected": {
        "result": "failure",
        "error_type": "ExplicitError"
      }
    },
    {
      "name": "simple_spend_valid",
      "description": "Spending validator with valid datum/redeemer",
      "script_path": "validators/simple_spend/script.flat",
      "plutus_version": "V2",
      "arguments": {
        "datum": "validators/simple_spend/datum.cbor",
        "redeemer": "validators/simple_spend/redeemer.cbor",
        "context": "validators/simple_spend/context.cbor"
      },
      "expected": {
        "result": "success"
      }
    }
  ]
}
```

### Initial Test Scripts to Create

Before writing any implementation code, create these minimal test scripts:

#### 1. `always_succeeds.uplc`
```uplc
-- Returns unit, always passes
(program 1.0.0 (con unit ()))
```

#### 2. `always_fails.uplc`
```uplc
-- Calls error builtin, always fails
(program 1.0.0 (error))
```

#### 3. `add_integers.uplc`
```uplc
-- Adds two integers: (1 + 3) = 4
(program 1.0.0 
  [[(builtin addInteger) (con integer 1)] (con integer 3)]
)
```

#### 4. `check_datum_redeemer.uplc` (Validator pattern)
```uplc
-- Spending validator: succeeds if datum == redeemer
(program 1.0.0
  (lam datum 
    (lam redeemer 
      (lam ctx 
        [[(builtin equalsData) datum] redeemer]
      )
    )
  )
)
```

### Compiling Test Scripts

Use the `pluton` CLI from pragma-org/uplc to compile:

```bash
# Parse and encode to FLAT
cargo run -p pluton -- encode --flat < always_succeeds.uplc > always_succeeds.flat

# Or use the Rust API directly in a build script
```

### Test Implementation Order (TDD Progression)

#### Phase 1: Core Evaluator (Red-Green-Refactor)

1. **Test**: `test_decode_valid_script` - Can decode FLAT bytes to Program
   - Write test expecting successful decode
   - Run → RED (no implementation)
   - Implement `decode_script()`
   - Run → GREEN

2. **Test**: `test_decode_invalid_bytes` - Fails gracefully on garbage
   - Write test expecting `ScriptDeserializationError`
   - Run → RED
   - Add error handling
   - Run → GREEN

3. **Test**: `test_eval_always_succeeds` - Evaluate trivial success
   - Write test expecting `EvalOutcome::Success`
   - Run → RED
   - Implement `evaluate_script()`
   - Run → GREEN

4. **Test**: `test_eval_always_fails` - Evaluate explicit error
   - Write test expecting `EvalOutcome::Failure { error: ExplicitError }`
   - Run → RED
   - Add error case handling
   - Run → GREEN

5. **Test**: `test_eval_budget_exceeded` - Budget enforcement
   - Write test with small budget
   - Run → RED
   - Implement budget limit checking
   - Run → GREEN

#### Phase 2: Argument Application

6. **Test**: `test_apply_validator_arguments` - Apply datum/redeemer/context
   - Write test for spending validator
   - Run → RED
   - Implement argument application
   - Run → GREEN

7. **Test**: `test_apply_policy_arguments` - Apply redeemer/context only
   - Write test for minting policy
   - Run → RED
   - Refine argument handling
   - Run → GREEN

#### Phase 3: Integration

8. **Test**: `test_phase2_validation_disabled` - Config flag respects disabled
   - Write test expecting Phase 1 only
   - Run → RED
   - Implement config flag check
   - Run → GREEN

9. **Test**: `test_phase2_validation_enabled` - Full validation flow
   - Write integration test with real transaction
   - Run → RED
   - Wire up to state.rs::validate()
   - Run → GREEN

10. **Test**: `test_parallel_script_evaluation` - Multiple scripts
    - Write test with multi-script block
    - Run → RED
    - Implement parallel execution
    - Run → GREEN

### Running Tests

```bash
# Run only Phase 2 validation tests
cargo test -p acropolis_module_tx_unpacker phase2

# Run with verbose output to see red/green cycle
cargo test -p acropolis_module_tx_unpacker phase2 -- --nocapture

# Run specific test
cargo test -p acropolis_module_tx_unpacker test_eval_always_succeeds
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
    
    #[error("Script {0} exceeded budget (cpu: {1}, mem: {2})")]
    BudgetExceeded(ScriptHash, i64, i64),
    
    #[error("Could not decode script {0}: {1}")]
    DecodeFailed(ScriptHash, String),
    
    #[error("Missing script for redeemer at index {0}")]
    MissingScript(u32),
    
    #[error("Missing datum {0}")]
    MissingDatum(DatumHash),
    
    #[error("Missing redeemer for script {0}")]
    MissingRedeemer(ScriptHash),
}

/// Execution budget (re-export from uplc or define simply)
#[derive(Debug, Clone, Copy)]
pub struct ExBudget {
    pub cpu: i64,
    pub mem: i64,
}
```

**Note**: We do NOT need separate `EvalRequest`, `EvalOutcome`, `ScriptEvaluation`, `ScriptError`, or `Phase2ValidationError` types. One error enum is sufficient.
