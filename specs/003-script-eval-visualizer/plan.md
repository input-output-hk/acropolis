# Implementation Plan: Script Evaluation Visualizer

**Branch**: `003-script-eval-visualizer` | **Date**: 2026-04-29 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/003-script-eval-visualizer/spec.md`

## Summary

Add a new Caryatid module `script_eval_visualizer` that subscribes to a new topic `cardano.utxo.phase2`, on which `utxo_state` (when configured to do so) publishes per-transaction phase-2 validation results. The module serves a static HTML/JS page (small embedded React-state app, no build tooling) and an SSE endpoint over `axum`; each script-evaluation arrival is fanned out as one SSE event per script. The page maintains a deque of the latest 1000 events (newest first) and renders them in a table with cexplorer.io links for block/tx. The publishing path inside `utxo_state` is gated by a config flag (default off) so disabled nodes pay no extra cost. Reuses the existing `evaluate_scripts` evaluation core, refactored to record per-script outcomes (mem/cpu actually consumed, success/failure) instead of only returning aggregate `Result<(), Phase2ValidationError>`.

Per L001 (PR #606): rollback semantics are explicit — published events are *evaluation events*, not chain state, so we do not emit "rescind" messages on rollback (also documented in spec assumptions). Per L003: the visualizer is downstream-only; it does not feed back into validation. Per L008: when refactoring `evaluate_scripts` to expose per-script outcomes, update inline doc comments to reflect the new shape.

## Technical Context

**Language/Version**: Rust workspace member, `edition = "2021"` (matches every other module in-tree; constitution says 2024 — see Constitution Check / Complexity Tracking).
**Primary Dependencies**:
- `caryatid_sdk` (workspace) — module macro, message bus, subscription
- `acropolis_common` (path) — `Message`, `BlockInfo`, `TxUTxODeltas`, `ScriptHash`, `RedeemerTag`, `PlutusVersion`, `ExUnits`
- `axum` (workspace) — HTTP/SSE server (same precedent as `mcp_server`)
- `tokio` + `tokio-stream` (workspace) — broadcast channel + SSE stream
- `serde` + `serde_json` (workspace) — JSON wire format for SSE payloads
- `tracing`, `anyhow`, `config`, `hex` — already in workspace

**Storage**: None. Live-only monitor; rows live in browser memory only. No Fjall.

**Testing**: `cargo test`; existing `validation_fixture!` macro in `utxo_state` for the phase-2 refactor; in-process integration test that constructs a `Phase2EvaluationResultsMessage`, publishes it on the bus, and asserts the visualizer's broadcast channel receives one fan-out event per script.

**Target Platform**: Linux (same as the rest of Acropolis); browser frontend on any modern desktop browser supporting `EventSource`.

**Project Type**: Single Rust workspace member, with embedded static HTML/JS asset (no build tooling).

**Performance Goals**:
- Per SC-001: first row visible to a connected browser within 3 s of evaluation completing under nominal load.
- Per SC-006: 1 h continuous operation at mainnet block rate without browser unresponsiveness.
- Internal broadcast channel sized to absorb a full block worth of evaluations without dropping connected clients (initial cap: 4096 events; lossy-newest semantics if a slow client falls behind — `tokio::sync::broadcast` natural behaviour).

**Constraints**:
- When the publishing toggle is **off** in `utxo_state`, the validation hot path takes a single-cmp early-out — no per-script bookkeeping, no allocation of result vectors, no message construction. Verifiable via SC-005.
- Constitution: no `unwrap`, no `panic!`. Result/`?` everywhere.
- Constitution: public types/functions have doc comments.
- SSE handler must not back-pressure the broadcast channel — slow clients are dropped, the bus continues.

**Scale/Scope**:
- One new module crate (~400–600 LOC including tests).
- One new `CardanoMessage::Phase2EvaluationResults` variant carrying `Phase2EvaluationResultsMessage`.
- Refactor `utxo_state::validations::phase_two::evaluate::evaluate_scripts` to surface per-script outcomes (success/failure + observed `ExUnits`).
- One frontend file `assets/index.html` (~150 LOC, embedded React via CDN-free `react.production.min.js` + `react-dom.production.min.js` either bundled or — to fully avoid build tooling — using a minimal hand-written `useState`-style table without React; final choice resolved in research.md).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Constitution rule | Status | Note |
|---|---|---|
| 1. Rust 2024 Edition | **Deviation tracked** | Every existing in-tree module is `edition = "2021"`. New module follows the workspace convention; tracked in Complexity Tracking. |
| 1. Tokio | Pass | Tokio is the async runtime; SSE stream uses `tokio_stream::wrappers::BroadcastStream`. |
| 1. thiserror/anyhow | Pass | `anyhow` for module init/run; `thiserror` for any local error type if needed. |
| 1. Serde + CBOR | Pass | Internal `Phase2EvaluationResultsMessage` derives Serde. SSE wire format is JSON (CBOR is for chain primitives, not human-readable telemetry — no constitution conflict). |
| 2. Modular | Pass | New isolated module crate; depends only on workspace `common` and `caryatid_sdk` (plus `axum`). |
| 2. lib.rs / internal split | Pass | `src/script_eval_visualizer.rs` is the lib entry; HTTP, SSE, stream-fan-out live in private submodules. |
| 2. Fjall v3 for DB | Pass (N/A) | No database. |
| 3. Idiomatic Rust / clippy clean | Pass | `make clippy` runs in CI; plan explicitly mandates `make all` clean. |
| 3. No unsafe | Pass | None planned. |
| 3. No `unwrap()` / no `panic!` | Pass | All `Result` paths use `?`; SSE serialization failures are logged and the bad event dropped (does not crash the stream). |
| 4. Doc comments on public items | Pass | Tasks call this out explicitly. |
| 5. TDD + integration tests | Pass | Phase-2 refactor: extend existing `phase2_evalute_test` cases to assert per-script outcome shape. New module: unit tests for fan-out + SSE serializer; one in-process integration test through Caryatid; one CI integration test under `tests/integration/`. |

**Result (initial)**: Pass with one tracked deviation. **Re-check after Phase 1**: still passing — Phase 1 introduces no new constitution-relevant decisions.

## Project Structure

### Documentation (this feature)

```text
specs/003-script-eval-visualizer/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/
│   ├── phase2-message.md    # cardano.utxo.phase2 message contract
│   ├── sse-stream.md        # /events SSE event contract
│   └── http-endpoints.md    # HTTP endpoints served by the module
├── checklists/
│   └── requirements.md
└── tasks.md             # Phase 2 output (/speckit.tasks — NOT created here)
```

### Source Code (repository root)

```text
modules/script_eval_visualizer/
├── Cargo.toml
├── src/
│   ├── script_eval_visualizer.rs   # lib.rs — module impl + #[module(...)] entry
│   ├── config.rs                   # config flag parsing (subscribe topic, bind addr/port)
│   ├── http.rs                     # axum router: GET / -> embedded index.html, GET /events -> SSE
│   ├── stream.rs                   # tokio broadcast channel + Phase2EvaluationResultsMessage
│   │                               #   -> per-script ScriptEvalEvent fan-out
│   └── assets/
│       └── index.html              # static HTML + small client app, embedded via include_str!
└── tests/
    └── stream_integration.rs       # in-process test: feed a fake message in, drain SSE events out

modules/utxo_state/
└── src/
    ├── utxo_state.rs               # ADD: phase2-publish-topic config + optional publisher;
    │                               #   gated by `publish-phase2-results` config flag (default false)
    ├── state.rs                    # WIRE: thread per-script outcomes up to publisher
    └── validations/
        └── phase_two/
            ├── evaluate.rs         # REFACTOR: evaluate_scripts returns per-script outcomes
            └── mod.rs              # REFACTOR: validate_tx_phase_two threads outcomes back up

common/
└── src/
    ├── messages.rs                 # ADD: Phase2EvaluationResultsMessage struct;
    │                               #   ADD: CardanoMessage::Phase2EvaluationResults variant
    └── validation/
        └── phase2.rs               # ADD: ScriptEvaluationOutcome (per-script result type)

processes/omnibus/
├── Cargo.toml                      # ADD: acropolis_module_script_eval_visualizer dep
├── omnibus.toml                    # ADD: [module.script-eval-visualizer] section;
│                                   #   ADD: publish-phase2-results = false under [module.utxo-state]
└── src/main.rs                     # ADD: ScriptEvalVisualizer::register(&mut process)

Cargo.toml                          # ADD: modules/script_eval_visualizer to workspace members
```

**Structure Decision**: Single Rust workspace, new module follows the shape of `mcp_server` (closest precedent: embedded axum HTTP server, registered through omnibus). The frontend is a single static HTML file embedded with `include_str!` — no separate `frontend/` tree, no build tooling.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| New crate uses `edition = "2021"` instead of constitution-mandated 2024 | Every other in-tree module is 2021. A single 2024 crate creates an inconsistent workspace and risks subtle proc-macro / `std` interaction differences with shared deps. | Migrating the whole workspace to 2024 is out of scope for this feature — would expand blast radius far beyond the visualizer. Deviation is documented and trivially reversible (one-line `Cargo.toml` change) when the workspace migrates. |
