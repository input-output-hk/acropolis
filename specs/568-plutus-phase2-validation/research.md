# Research: Plutus Phase 2 Validation Integration

**Feature**: 568-plutus-phase2-validation  
**Date**: 2026-02-06

## Executive Summary

Acropolis currently implements **Phase 1 validation only**. Phase 2 (Plutus script execution) is not implemented. The integration point for Phase 2 validation is well-defined and will fit naturally after existing Phase 1 validation in the `tx_unpacker` module.

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

This follows the existing pattern where `TransactionValidationError` wraps both Phase 1 and Phase 2 errors.

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
