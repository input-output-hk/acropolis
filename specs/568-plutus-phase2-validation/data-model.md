# Data Model: Plutus Phase 2 Validation

**Feature**: 568-plutus-phase2-validation  
**Date**: 2026-02-06

## Overview

This document defines the minimal types needed for Phase 2 validation. The design principle is: **use existing pallas types wherever possible** and only define new types when necessary.

```
┌─────────────────────────────────────────────────────────────┐
│                    Phase 2 Validation Flow                   │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Transaction ──▶ Extract Scripts & Redeemers                │
│       │                                                     │
│       ▼                                                     │
│  Build ScriptContext (CBOR PlutusData)                      │
│       │                                                     │
│       ▼                                                     │
│  evaluate_script() ──▶ Result<ExBudget, Phase2Error>        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## New Types (3 total)

### 1. ExBudget

Execution resource tracking. Simple struct to track consumed vs allocated budget.

```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct ExBudget {
    pub cpu: i64,
    pub mem: i64,
}
```

**Note**: This may be re-exported from uplc-turbo if it provides one.

---

### 2. Phase2Error

Single error enum for all Phase 2 failures.

```rust
#[derive(Debug, thiserror::Error)]
pub enum Phase2Error {
    /// Script explicitly called `error` builtin
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
```

**Design decision**: One error type, not separate `ScriptError` and `Phase2ValidationError`. Simpler.

---

### 3. ScriptPurpose

Identifies why a script is being evaluated. Used to build the correct ScriptContext.

```rust
#[derive(Debug, Clone)]
pub enum ScriptPurpose {
    Spending(TransactionInput),
    Minting(PolicyId),
    Certifying { index: u32 },
    Rewarding(RewardAddress),
    Voting(Voter),         // V3 only
    Proposing { index: u32 }, // V3 only
}
```

---

## Re-used Existing Types

These types already exist in pallas or common crates—do NOT redefine them:

| Type | Source | Usage |
|------|--------|-------|
| `PlutusScript` | `pallas_primitives` | Script bytes and version |
| `PlutusVersion` | `pallas_primitives` | V1, V2, V3 enum |
| `PlutusData` | `pallas_primitives` | Datum, redeemer, context encoding |
| `Redeemer` | `pallas_primitives` | Redeemer with tag and index |
| `ScriptHash` | `pallas_crypto` | 28-byte script identifier |
| `DatumHash` | `pallas_crypto` | 32-byte datum hash |
| `CostModels` | protocol params | Maps PlutusVersion → `Vec<i64>` |

---

## ScriptContext Building

ScriptContext is NOT a Rust struct—it's a `PlutusData` value built dynamically:

```rust
// Build ScriptContext as PlutusData for script evaluation
fn build_script_context(
    tx: &MultiEraTx,
    purpose: &ScriptPurpose,
    version: PlutusVersion,
) -> PlutusData {
    // ScriptContext = Constr 0 [TxInfo, ScriptPurpose]
    // TxInfo varies by version (V1 vs V2 vs V3)
    // Use pallas_codec::utils::PlutusData::constr()
}
```

**Note**: The exact encoding follows CIP-0035 (V1/V2) and CIP-0069 (V3). Refer to pallas or cardano-ledger for canonical encodings.

---

## Protocol Parameters Used

| Field | Usage |
|-------|-------|
| `max_tx_ex_units.steps` | Max CPU budget per transaction |
| `max_tx_ex_units.mem` | Max memory budget per transaction |
| `plutus_v1_cost_model` | Cost coefficients for V1 evaluation |
| `plutus_v2_cost_model` | Cost coefficients for V2 evaluation |
| `plutus_v3_cost_model` | Cost coefficients for V3 evaluation |

---

## What We Explicitly Don't Define

The following types were considered but **removed** as unnecessary abstractions:

| Type | Why Not Needed |
|------|----------------|
| `EvalRequest` | Just pass parameters directly to `evaluate_script()` |
| `EvalOutcome` | Return `Result<ExBudget, Phase2Error>` instead |
| `ScriptError` | Merged into `Phase2Error` |
| `ScriptEvaluation` | Intermediate wrapper, not needed |
| `TxInfo` struct | Build `PlutusData` directly, no intermediate Rust struct |
| `Phase2Config` | Just use `enabled: bool` in existing config |
| `ScriptContext` struct | Build `PlutusData` directly, no intermediate Rust struct |
