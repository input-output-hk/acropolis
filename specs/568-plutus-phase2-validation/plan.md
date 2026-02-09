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
│   │   └── phase2/              # NEW: Phase 2 validation
│   │       ├── mod.rs           # Phase 2 public API
│   │       ├── evaluator.rs     # uplc wrapper
│   │       ├── context.rs       # ScriptContext builder
│   │       └── error.rs         # Phase2ValidationError
│   └── ...
├── Cargo.toml                    # Add uplc-turbo dependency
└── tests/
    └── phase2_validation_test.rs # Integration tests

common/src/
└── validation.rs                 # Existing: add Phase2ValidationError
```

**Structure Decision**: Integrate within existing tx_unpacker module to avoid message bus complexity. New `validations/phase2/` subdirectory mirrors existing Phase 1 structure.

## Complexity Tracking

> No violations - complexity tracking not required.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| [e.g., 4th project] | [current need] | [why 3 projects insufficient] |
| [e.g., Repository pattern] | [specific problem] | [why direct DB access insufficient] |
