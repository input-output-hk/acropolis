# Implementation Plan: Datum Lifecycle Management

**Branch**: `568-datum-lifecycle` | **Date**: 2026-02-12 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/568-datum-lifecycle/spec.md`

## Summary

Complete the Phase 2 Plutus validation by enabling datum resolution from the UTxO store, constructing proper ScriptContext/TxInfo per Plutus version, and integrating Phase 2 validation into the `utxo_state` module where resolved UTxO data is available. Extract the Phase 2 evaluator into a shared library crate for reuse.

## Technical Context

**Language/Version**: Rust 2024 Edition  
**Primary Dependencies**: uplc-turbo (pragma-org/uplc, pinned commit), pallas, tokio, rayon, caryatid  
**Storage**: Fjall v3 (immutable UTxOs), DashMap/HashMap (volatile UTxOs)  
**Testing**: cargo test, integration tests with mainnet block fixtures  
**Target Platform**: Linux server (x86_64), macOS (aarch64 dev)  
**Project Type**: Single Rust workspace with multiple crates  
**Performance Goals**: Datum resolution < 1ms overhead per transaction (SC-006); individual script evaluation < 100ms (existing SC-001)  
**Constraints**: Phase 2 must run sequentially after Phase 1 per transaction (L007); scripts within a transaction can run in parallel (L008)  
**Scale/Scope**: ~7 files modified/created, 1 new crate

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Constitution Rule | Status | Notes |
|-------------------|--------|-------|
| Rust 2024 Edition | ✅ Pass | All new code uses Rust 2024 edition |
| Tokio async runtime | ✅ Pass | UTxO state module already uses tokio |
| thiserror/anyhow error handling | ✅ Pass | New error types use thiserror |
| Serde + CBOR serialization | ✅ Pass | ScriptContext built as CBOR PlutusData |
| Modular architecture | ✅ Pass | Evaluation extracted to shared crate; clean module boundaries |
| Strict lib.rs / internal separation | ✅ Pass | Public API via lib.rs, internals in submodules |
| Fjall v3 for database | ✅ Pass | UTxO immutable store already uses Fjall |
| Idiomatic Rust / clippy | ✅ Pass | All code passes `clippy --all-targets -- -D warnings` |
| No unwrap() / no panic() | ✅ Pass | All error paths use Result + ? |
| Doc comments on public types | ✅ Pass | All public types and functions documented |
| TDD workflow | ✅ Pass | Tests written before/alongside implementation |
| Integration tests for CI/CD | ✅ Pass | Mainnet block fixtures for regression testing |

## Project Structure

### Documentation (this feature)

```text
specs/568-datum-lifecycle/
├── plan.md              # This file
├── research.md          # Phase 0 output — architecture decisions
├── data-model.md        # Phase 1 output — entity relationships
├── quickstart.md        # Phase 1 output — getting started guide
├── contracts/           # Phase 1 output — API contracts
│   └── resolved-utxo.md # ResolvedUTxO interface contract
└── checklists/
    └── requirements.md  # Spec quality validation
```

### Source Code (repository root)

```text
# New shared crate
modules/plutus_validation/
├── Cargo.toml
└── src/
    ├── lib.rs               # Public API: evaluate_scripts, ScriptInput, Phase2Result
    ├── evaluator.rs         # Core evaluation logic (moved from tx_unpacker/validations/phase2.rs)
    ├── arena.rs             # Arena pool + thread pool (moved from tx_unpacker/validations/phase2.rs)
    ├── script_context/
    │   ├── mod.rs           # Version-dispatching ScriptContext builder
    │   ├── v1.rs            # PlutusV1 TxInfo/ScriptContext CBOR construction
    │   ├── v2.rs            # PlutusV2 TxInfo/ScriptContext CBOR construction
    │   └── v3.rs            # PlutusV3 TxInfo/ScriptContext/ScriptInfo CBOR construction
    └── datum.rs             # Datum resolution logic (resolve_datum, validate_datum_hash)

# Modified files
modules/utxo_state/
├── src/
│   ├── state.rs             # Add Phase 2 call in validate_block_utxos loop
│   └── validations/
│       ├── mod.rs           # Wire Alonzo/Babbage validation
│       └── alonzo/
│           └── utxow.rs     # Implement datum validation rules (currently stubs)

modules/tx_unpacker/
├── src/
│   ├── state.rs             # Delegate to shared crate, remove UTxO-dependent code
│   └── validations/
│       └── phase2.rs        # Thin wrapper delegating to shared crate

common/src/
└── ledger_state.rs          # Extend TxUTxODeltas with raw_tx field

codec/src/
└── tx.rs                    # Populate raw_tx in TxUTxODeltas

tests/
├── fixtures/                # Mainnet block fixtures with script transactions
└── integration/             # Integration tests for datum resolution
```

**Structure Decision**: Single workspace with a new `acropolis_module_plutus_validation` crate. The crate boundary separates evaluation logic (pure functions) from state management (UTxO lookups). This follows the existing pattern where `acropolis_common` provides shared types and individual modules provide domain logic.

## Key Architectural Decisions

### AD-1: Phase 2 runs in utxo_state (from RQ-1)

The `validate_block_utxos` loop in `utxo_state/src/state.rs` already:
1. Collects all input UTxOs into a HashMap
2. Iterates transactions sequentially
3. Runs Phase 1 validation per-tx
4. Accumulates outputs between txs (enables FR-009 intra-block deps)

Phase 2 inserts after step 3, using the same UTxO HashMap for datum resolution and ScriptContext construction. The Phase 2 result (pass/fail) controls whether inputs or collateral are applied.

### AD-2: Shared evaluation crate (from RQ-1 + L006)

The 937-line Phase 2 evaluator (arena pool, thread pool, script evaluation) moves to `modules/plutus_validation/` so both `utxo_state` and `tx_unpacker` can use it. Per L006, this avoids duplicating evaluation logic.

### AD-3: Raw tx bytes in TxUTxODeltas (from RQ-2)

`TxUTxODeltas` gains an `Option<Vec<u8>>` field for raw transaction bytes. This enables `utxo_state` to re-extract script bytecodes, witness-set datums, and redeemers needed for Phase 2. The field is `Option` for backward compatibility.

### AD-4: Version-aware ScriptContext (from RQ-3 + RQ-4)

ScriptContext construction is version-specific:
- V1/V2: `program.apply(datum).apply(redeemer).apply(context)` — context = `[TxInfo, ScriptPurpose]`
- V3: `program.apply(context)` only — context = `[TxInfo, Redeemer, ScriptInfo]` with datum embedded

### AD-5: Datum resolution algorithm (from RQ-5)

1. Look up consumed UTxO → get `UTXOValue.datum`
2. `Inline(bytes)` → use directly
3. `Hash(h)` → find in `tx.plutus_data` where hash matches → validate hash → use bytes
4. `None` + V1/V2 → error
5. `None` + V3 → allowed (CIP-0069)

## Post-Design Constitution Re-Check

*Re-evaluated after Phase 1 design completion.*

| Constitution Rule | Status | Design Artifact |
|-------------------|--------|-----------------|
| Rust 2024 Edition | ✅ Pass | New crate uses edition = "2024" in Cargo.toml |
| Tokio async runtime | ✅ Pass | utxo_state integration uses existing tokio context |
| thiserror/anyhow errors | ✅ Pass | `Phase2Error` uses `#[derive(thiserror::Error)]` |
| Serde + CBOR serialization | ✅ Pass | ScriptContext built as CBOR PlutusData (data-model.md) |
| Modular architecture | ✅ Pass | Shared crate with clean public API (contracts/resolved-utxo.md) |
| Strict lib.rs separation | ✅ Pass | `lib.rs` re-exports public types; internals in submodules |
| Fjall v3 for database | ✅ Pass | UTxO lookups use existing Fjall store in utxo_state |
| Clippy compliance | ✅ Pass | CI gate: `clippy --all-targets -- -D warnings` |
| No unwrap() / panic() | ✅ Pass | All error paths return `Result<_, Phase2Error>` |
| Doc comments | ✅ Pass | All public types/functions documented (see contracts) |
| TDD workflow | ✅ Pass | Tests written first using mainnet fixtures (quickstart.md) |
| Integration tests for CI | ✅ Pass | Mainnet block fixtures for regression testing |

**Result**: All 12 constitution rules pass. No violations to justify.
