# Quickstart: Datum Lifecycle Management

## Prerequisites

- Rust 2024 edition toolchain (rustup default stable)
- The `568-datum-lifecycle` branch checked out
- Working `cargo build` on the acropolis workspace

## Build

```bash
# Build the new shared crate
cargo build -p acropolis_module_plutus_validation

# Build the modified utxo_state module
cargo build -p acropolis_module_utxo_state

# Build everything
cargo build
```

## Test

```bash
# Run unit tests for the new crate
cargo test -p acropolis_module_plutus_validation

# Run utxo_state tests (includes Phase 2 integration)
cargo test -p acropolis_module_utxo_state

# Run tx_unpacker tests (existing Phase 2 tests, now using shared crate)
cargo test -p acropolis_module_tx_unpacker

# Run all tests
cargo test

# Clippy (required to pass per constitution)
cargo clippy --all-targets --all-features -- -D warnings
```

## Key Files to Read

| File | Purpose |
|------|---------|
| `modules/plutus_validation/src/lib.rs` | Public API — start here |
| `modules/plutus_validation/src/evaluator.rs` | Core UPLC evaluation (moved from tx_unpacker) |
| `modules/plutus_validation/src/script_context/mod.rs` | Version-dispatching ScriptContext builder |
| `modules/plutus_validation/src/datum.rs` | Datum resolution algorithm |
| `modules/utxo_state/src/state.rs` | Phase 2 integration point in `validate_block_utxos` |
| `common/src/ledger_state.rs` | `TxUTxODeltas` with `raw_tx` field |

## Architecture Overview

```
tx_unpacker                       utxo_state
    │                                 │
    │ (produces TxUTxODeltas          │ (has UTxO store,
    │  with raw_tx bytes)             │  runs Phase 1 + Phase 2)
    │                                 │
    └────────────────┬────────────────┘
                     │ depends on
                     ▼
          plutus_validation (shared crate)
          ┌──────────────────────────┐
          │ evaluate_transaction_p2  │
          │ build_script_context     │
          │ resolve_datum            │
          │ arena_pool / thread_pool │
          └──────────────────────────┘
                     │ depends on
                     ▼
               uplc-turbo (UPLC evaluator)
```

## Development Workflow

1. **TDD**: Write tests first in `modules/plutus_validation/src/` using mainnet fixtures
2. **Unit test**: `cargo test -p acropolis_module_plutus_validation`
3. **Integration test**: `cargo test -p acropolis_module_utxo_state`
4. **Clippy**: `cargo clippy --all-targets --all-features -- -D warnings`
5. **Benchmark**: `cargo test -p acropolis_module_plutus_validation --release -- benchmark --nocapture`
