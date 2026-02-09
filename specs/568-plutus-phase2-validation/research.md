# Research: Plutus Phase 2 Validation Integration

**Feature**: 568-plutus-phase2-validation  
**Date**: 2026-02-06

## Executive Summary

Acropolis currently implements **Phase 1 validation only**. Phase 2 (Plutus script execution) is not implemented. The integration point for Phase 2 validation is well-defined and will fit naturally after existing Phase 1 validation in the `tx_unpacker` module.

The `uplc-turbo` crate from pragma-org provides an arena-based Plutus evaluator with the following key characteristics:
- Arena allocator (`bumpalo`) for constant-memory execution
- Support for Plutus V1, V2, V3 via `PlutusVersion` enum
- Cost model support via `eval_with_params()` for protocol parameter integration
- FLAT encoding/decoding for script bytecode

---

## Question 1: Where is Phase 1 validation implemented?

### Decision: `modules/tx_unpacker/src/validations/`

### Rationale

Phase 1 validation is implemented in the **tx_unpacker module** with the following structure:

| File | Responsibility |
|------|----------------|
| `validations/mod.rs` | Entry point: `validate_tx()` function routes to era-specific validation |
| `validations/alonzo/utxow.rs` | Alonzo-era UTxOW rules (script integrity hash) |
| `validations/babbage/utxow.rs` | Babbage-era UTxOW rules (reference scripts, inline datums) |
| `validations/conway/utxow.rs` | Conway-era UTxOW rules |
| `validations/shelley/` | Base Shelley rules reused by later eras |

The validation is invoked from `state.rs::validate()` which is called from the main module loop in `tx_unpacker.rs`.

### Evidence

From `modules/tx_unpacker/src/validations/mod.rs` lines 13-50:
```rust
pub fn validate_tx(
    raw_tx: &[u8],
    genesis_delegs: &GenesisDelegates,
    shelley_params: &Option<ShelleyParams>,
    current_slot: u64,
    era: Era,
) -> Result<(), Box<TransactionValidationError>> {
    // ... decodes tx and routes to era-specific validation
    match era {
        Era::Alonzo => validate_alonzo_compatible_tx(...),
        Era::Babbage => validate_babbage_tx(...),
        Era::Conway => validate_conway_tx(...),
        // ...
    }
}
```

---

## Question 2: What Phase 1 validations exist for Plutus scripts?

### Decision: Only structural validation, no execution

### Rationale

Current Phase 1 validation for Plutus-containing transactions includes:

1. **Script Integrity Hash Validation** (Alonzo+)
   - Verifies the `script_data_hash` in transaction body matches computed hash of redeemers + datums + cost model
   - Located in `validations/alonzo/utxow.rs::validate_script_integrity_hash()`

2. **Missing/Extra Redeemer Checks**
   - `UTxOWValidationError::MissingRedeemers`
   - `UTxOWValidationError::ExtraRedeemers`

3. **Datum Validation**
   - `UTxOWValidationError::MissingRequiredDatums`
   - `UTxOWValidationError::NotAllowedSupplementalDatums`

### What's NOT implemented (Phase 2)

- Plutus script bytecode execution
- Budget consumption tracking
- Script success/failure evaluation
- ScriptContext construction

---

## Question 3: Where should Phase 2 validation be integrated?

### Decision: In `state.rs::validate()` after Phase 1 passes

### Rationale

The `State::validate()` method in `modules/tx_unpacker/src/state.rs` is the natural integration point:

```rust
pub fn validate(
    &self,
    block_info: &BlockInfo,
    txs_msg: &RawTxsMessage,
    genesis_delegs: &GenesisDelegates,
) -> Result<(), Box<ValidationError>> {
    let mut bad_transactions = Vec::new();
    for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
        // Phase 1 validation (existing)
        if let Err(e) = self.validate_transaction(block_info, raw_tx, genesis_delegs) {
            bad_transactions.push((tx_index, *e));
        }
        
        // INTEGRATION POINT: Phase 2 validation goes here
        // Only run if Phase 1 passed and tx contains scripts
    }
    // ...
}
```

### Alternatives Considered

1. **Separate module**: Creating a new `plutus_validator` module
   - Pro: Clean separation of concerns
   - Con: Would require passing validation context through message bus
   - **Rejected**: Adds unnecessary complexity

2. **In tx_unpacker.rs main loop**: After validation outcomes publish
   - Pro: Could run asynchronously
   - Con: Validation results wouldn't be included in same publish
   - **Rejected**: Would fragment validation reporting

---

## Question 4: What data is needed for Phase 2 validation?

### Decision: Transaction, UTxO context, and protocol parameters

### Required Data

| Data | Source | Available? |
|------|--------|-----------|
| Plutus script bytecode | Transaction witness set | ✅ Via pallas |
| Redeemers | Transaction witness set | ✅ Via pallas |
| Datums | Transaction witness set + UTxO | ✅ Via pallas |
| Script purpose (Spend/Mint/Cert/Reward) | Redeemer pointer | ✅ Via pallas |
| Cost model parameters | Protocol params | ✅ In ProtocolParams |
| Execution budget limits | Protocol params | ✅ `max_tx_ex_units` |
| Transaction context (for ScriptContext) | Transaction body | ✅ Need to construct |
| Input UTxOs (for spending scripts) | UTxO state | ⚠️ May need resolver |

### Key Finding: UTxO Resolution

For spending scripts, we need the UTxO being consumed to:
1. Extract inline datum (Babbage+)
2. Verify script hash matches

The `tx_unpacker` does not currently have access to UTxO state. Options:
1. Pass UTxO resolver to validation
2. Require UTxO data in message
3. Look up via message bus query

---

## Question 5: What error types are needed for Phase 2?

### Decision: Add `Phase2ValidationError` enum

### Rationale

Create a new error enum parallel to `Phase1ValidationError`:

```rust
pub enum Phase2ValidationError {
    /// Script execution failed
    ScriptFailure {
        script_hash: ScriptHash,
        redeemer_pointer: RedeemerPointer,
        error: String,
    },
    
    /// Script exceeded budget
    ExceededBudget {
        script_hash: ScriptHash,
        consumed: ExUnits,
        limit: ExUnits,
    },
    
    /// Script deserialization failed
    ScriptDeserializationError {
        script_hash: ScriptHash,
        reason: String,
    },
    
    /// Missing script for redeemer
    MissingScript {
        script_hash: ScriptHash,
    },
}
```

This establishes a clear error type for Phase 2 so that `TransactionValidationError` can be extended (or a new wrapper introduced) to include both Phase 1 and Phase 2 validation errors.

---

## Question 6: How to handle configuration flag?

### Decision: Add to module configuration in omnibus.toml

### Rationale

Following the existing pattern for module configuration:

```toml
[module.tx-unpacker]
# ... existing config ...
phase2_validation_enabled = false  # Default: disabled
```

Access via `Config` in module initialization, similar to how other modules handle feature flags.

---

## Summary of Integration Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     tx_unpacker module                       │
├─────────────────────────────────────────────────────────────┤
│  tx_unpacker.rs                                             │
│    ├── Receives CardanoMessage::ReceivedTxs                 │
│    ├── Calls state.validate()                               │
│    └── Publishes validation outcomes                        │
│                                                             │
│  state.rs                                                   │
│    └── validate()                                           │
│        ├── Phase 1: validate_transaction() [EXISTING]       │
│        │   └── validations/mod.rs::validate_tx()           │
│        │                                                    │
│        └── Phase 2: validate_plutus_scripts() [NEW]        │
│            └── Call pragma-org/uplc evaluator              │
│                ├── For each script in tx                   │
│                ├── Build ScriptContext                     │
│                ├── Execute with budget limit               │
│                └── Collect results                         │
├─────────────────────────────────────────────────────────────┤
│  Configuration: phase2_validation_enabled (default: false)  │
└─────────────────────────────────────────────────────────────┘
```
---

## Question 7: How does the uplc-turbo crate API work?

### Decision: Use arena-based evaluation with `Program::eval_with_params()`

### Rationale

The `uplc-turbo` crate (dependency name in Cargo.toml) provides a high-performance Plutus evaluator with the following key API components:

#### Core Types

```rust
// Arena allocator for all script data (from bumpalo)
use uplc_turbo::arena::Arena;
use uplc_turbo::bumpalo::Bump;

// Program representation
use uplc_turbo::program::Program;
use uplc_turbo::binder::DeBruijn;

// Plutus version selection
use uplc_turbo::machine::PlutusVersion; // V1, V2, V3

// Execution budget
use uplc_turbo::machine::ExBudget;

// Evaluation result
use uplc_turbo::machine::EvalResult;
```

#### Script Decoding (FLAT format)

Scripts on Cardano are CBOR-wrapped FLAT-encoded bytecode. The decode flow:

```rust
use uplc_turbo::flat;

// Create arena with pre-allocated capacity (1MB recommended)
let bump = Bump::with_capacity(1_024_000);
let arena = Arena::from_bump(bump);

// CBOR-unwrap to get FLAT bytes (scripts are double-wrapped)
let flat_bytes: &[u8] = unwrap_cbor_script(&cbor_script_bytes);

// Decode FLAT to Program
let program: &Program<DeBruijn> = flat::decode(&arena, flat_bytes)?;
```

#### Script Evaluation

The `Program` type provides several evaluation methods:

```rust
// Simple evaluation (uses V3, unlimited budget)
let result: EvalResult = program.eval(&arena);

// Evaluation with explicit Plutus version
let result = program.eval_version(&arena, PlutusVersion::V2);

// Evaluation with version and initial budget limit
let result = program.eval_version_budget(
    &arena,
    PlutusVersion::V2,
    ExBudget { cpu: 10_000_000_000, mem: 10_000_000 }
);

// Evaluation with protocol parameters cost model (RECOMMENDED)
let result = program.eval_with_params(
    &arena,
    PlutusVersion::V2,
    &cost_model_params,  // &[i64] from protocol parameters
    ExBudget { cpu: max_cpu, mem: max_mem }
);
```

#### Result Handling

```rust
pub struct EvalResult<'a, V> {
    pub term: Result<&'a Term<'a, V>, MachineError<'a, V>>,
    pub info: MachineInfo,
}

pub struct MachineInfo {
    pub consumed_budget: ExBudget,  // Actual CPU/mem used
    pub logs: Vec<String>,          // Debug trace output
}

// Check if script succeeded
match result.term {
    Ok(term) => {
        // Script succeeded - check if it returned unit or expected value
        // For validators, success means returning () unit
    }
    Err(MachineError::ExplicitErrorTerm) => {
        // Script explicitly failed via `error` builtin
    }
    Err(MachineError::OutOfExError(budget)) => {
        // Script exceeded execution budget
    }
    Err(e) => {
        // Other evaluation error
    }
}
```

#### Memory Management

The arena allocator is key to constant memory:

```rust
// Create arena once per block (or reuse with reset)
let mut arena = Arena::from_bump(Bump::with_capacity(1_024_000));

for script in block_scripts {
    let program = flat::decode(&arena, &script.bytes)?;
    let result = program.eval_with_params(&arena, ...);
    // Process result...
    
    // Reset arena between scripts for constant memory
    arena.reset();
}
```

### Applying Script Arguments

For Plutus validators, the script must be applied to its arguments (datum, redeemer, script context) before evaluation:

```rust
// Build datum, redeemer, context as PlutusData
let datum_term = Term::constant(&arena, Constant::data(&arena, datum_data));
let redeemer_term = Term::constant(&arena, Constant::data(&arena, redeemer_data));
let context_term = Term::constant(&arena, Constant::data(&arena, script_context));

// Apply arguments to script program
let applied = program
    .apply(&arena, datum_term)
    .apply(&arena, redeemer_term)
    .apply(&arena, context_term);

// Now evaluate
let result = applied.eval_with_params(&arena, plutus_version, &cost_model, budget);
```

### Cost Model Parameters

The cost model is a `&[i64]` array from protocol parameters. Map Acropolis `ProtocolParams` fields:

- `plutus_v1_cost_model` → V1 scripts
- `plutus_v2_cost_model` → V2 scripts  
- `plutus_v3_cost_model` → V3 scripts

Each is a vector of ~150-200 integers defining operation costs.

---

## Question 8: How to build ScriptContext?

### Decision: Construct PlutusData representation of transaction context

### Rationale

The ScriptContext is a PlutusData structure containing:

1. **TxInfo**: Transaction body info (inputs, outputs, mint, fee, etc.)
2. **ScriptPurpose**: Why the script is running (Spending, Minting, Certifying, Rewarding, Voting, Proposing)

For spending scripts:
```rust
let script_context = PlutusData::constr(&arena, 0, &[
    tx_info_data,      // Full transaction context
    script_purpose,    // Constr for purpose type
]);
```

The tx_info structure varies by Plutus version:
- V1: Limited fields, basic tx info
- V2: Adds reference inputs, inline datums
- V3: Adds governance fields, voting

### Key Implementation Tasks

1. **TxInfo builder**: Convert pallas `TransactionBody` to PlutusData
2. **ScriptPurpose builder**: Map redeemer pointer to purpose type
3. **Version dispatcher**: Select correct structure based on PlutusVersion

---

## Summary: Integration Code Pattern

```rust
/// Evaluate a single Plutus script
pub fn evaluate_script(
    arena: &Arena,
    script_bytes: &[u8],
    plutus_version: PlutusVersion,
    datum: Option<&PlutusData>,
    redeemer: &PlutusData,
    script_context: &PlutusData,
    cost_model: &[i64],
    budget: ExBudget,
) -> Result<EvalResult, Phase2ValidationError> {
    // Decode script
    let program: &Program<DeBruijn> = uplc_turbo::flat::decode(arena, script_bytes)
        .map_err(|e| Phase2ValidationError::ScriptDeserializationError {
            reason: e.to_string(),
        })?;
    
    // Apply arguments based on script type
    let applied = if let Some(datum) = datum {
        // Spending script: datum, redeemer, context
        program
            .apply(arena, Term::constant(arena, Constant::data(arena, datum)))
            .apply(arena, Term::constant(arena, Constant::data(arena, redeemer)))
            .apply(arena, Term::constant(arena, Constant::data(arena, script_context)))
    } else {
        // Minting/other script: redeemer, context
        program
            .apply(arena, Term::constant(arena, Constant::data(arena, redeemer)))
            .apply(arena, Term::constant(arena, Constant::data(arena, script_context)))
    };
    
    // Evaluate with budget limit
    let result = applied.eval_with_params(arena, plutus_version, cost_model, budget);
    
    Ok(result)
}
```