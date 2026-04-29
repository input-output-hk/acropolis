---

description: "Task list for the Script Evaluation Visualizer feature"
---

# Tasks: Script Evaluation Visualizer

**Input**: Design documents from `/specs/003-script-eval-visualizer/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: Constitution rule 5 mandates TDD. Test tasks are included for each story; write each test FIRST and confirm it FAILS before the implementation task that satisfies it.

**Organization**: Tasks are grouped by user story. The Foundational phase (Phase 2) lands the publishing path's data types and the `evaluate_scripts` refactor — every story builds on it. After Phase 2, US1 (MVP) is independently demonstrable; US2 and US3 layer additional behaviour on top without changing US1's contract.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no dependency on incomplete tasks)
- **[Story]**: which user story (US1/US2/US3) — only used in user-story phases
- All paths are absolute from the repo root `/home/hypo/works/input-output-hk/acropolis`.

## Path Conventions

Single Rust workspace. New module at `modules/script_eval_visualizer/`. Touched code in `common/`, `modules/utxo_state/`, and `processes/omnibus/`.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: scaffold the new crate and register it in the workspace.

- [X] T001 Create directory tree `modules/script_eval_visualizer/{src/assets,tests}` and a minimal `modules/script_eval_visualizer/Cargo.toml` (package name `acropolis_module_script_eval_visualizer`, edition `2021`, `[lib] path = "src/script_eval_visualizer.rs"`, deps: `acropolis_common = { path = "../../common" }`, `caryatid_sdk` (workspace), `axum` (workspace), `tokio` + `tokio-stream` (workspace), `serde` + `serde_json` (workspace), `anyhow`, `config`, `tracing`, `hex`).
- [X] T002 Add `"modules/script_eval_visualizer"` to `members` in `/home/hypo/works/input-output-hk/acropolis/Cargo.toml` (alphabetical position next to `mcp_server`).
- [X] T003 [P] Create empty placeholder lib at `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/script_eval_visualizer.rs` with `//! Acropolis Script Evaluation Visualizer module` doc-comment header so the workspace builds before further work.
- [X] T004 [P] Create placeholder `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/assets/index.html` containing only `<!doctype html><meta charset="utf-8"><title>Script Evaluation Visualizer</title>` so `include_str!` calls in later tasks can compile incrementally.

**Checkpoint**: `cargo build -p acropolis_module_script_eval_visualizer` succeeds with no symbols.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: land the per-script outcome data types in `common`, refactor `evaluate_scripts` to surface them, and thread them up to `utxo_state.rs::run`. Nothing in any user story compiles without these.

**⚠️ CRITICAL**: No user-story phase can start until Phase 2 is complete.

- [X] T005 [P] Add `ScriptEvaluationOutcome` to `/home/hypo/works/input-output-hk/acropolis/common/src/validation/phase2.rs` with the seven fields documented in [data-model.md §1](./data-model.md). Derive `Debug, Clone, serde::Serialize, serde::Deserialize`. Add `///` doc comments per constitution rule 4.
- [X] T006 [P] Add `Phase2EvaluationResultsMessage` struct (per [data-model.md §2](./data-model.md)) and a new variant `Phase2EvaluationResults(Phase2EvaluationResultsMessage)` to the `CardanoMessage` enum in `/home/hypo/works/input-output-hk/acropolis/common/src/messages.rs`. Re-export `ScriptEvaluationOutcome` via the standard `use crate::validation::...` pattern already used in that file. Doc comments required.
- [X] T007 Refactor `/home/hypo/works/input-output-hk/acropolis/modules/utxo_state/src/validations/phase_two/evaluate.rs::evaluate_scripts` to return `(Vec<ScriptEvaluationOutcome>, Result<(), Phase2ValidationError>)` instead of just `Result`. Internal change: replace `try_for_each` with `map(...).collect::<Vec<_>>()` per script context, capturing observed `mem`/`cpu` from the UPLC machine result alongside success/failure; aggregate the `Err`s with the same precedence as before so existing tests still see the same overall `Result`. Update doc comment per L008.
- [X] T008 Update `/home/hypo/works/input-output-hk/acropolis/modules/utxo_state/src/validations/phase_two/mod.rs::validate_tx_phase_two` to return `(Vec<ScriptEvaluationOutcome>, Result<(), Phase2ValidationError>)`. Then update `/home/hypo/works/input-output-hk/acropolis/modules/utxo_state/src/validations/mod.rs::validate_tx` to thread outcomes back to its caller as `(Vec<ScriptEvaluationOutcome>, Result<(), Box<TransactionValidationError>>)` (empty vec when phase-2 was skipped: pre-Alonzo, no redeemers, etc.).
- [X] T009 Update `/home/hypo/works/input-output-hk/acropolis/modules/utxo_state/src/state.rs::State::validate` to collect a `Vec<(TxHash, u32, bool, Vec<ScriptEvaluationOutcome>)>` (one tuple per tx that ran phase-2 with non-empty outcomes) and return it from `validate(...)` in addition to the existing return value. Empty vec is the common case; allocation MUST be skipped when the caller signals the publish flag is off (see T013).
- [X] T010 [P] Extend the existing `phase2_evalute_test` cases in `/home/hypo/works/input-output-hk/acropolis/modules/utxo_state/src/validations/phase_two/evaluate.rs` (the `#[cfg(test)] mod tests`) to assert: (a) the returned `Vec<ScriptEvaluationOutcome>` has exactly one entry per Plutus script in the fixture, (b) `is_success` matches the expected outcome, (c) `ex_units_used.mem ≤ ex_units_budget.mem` and `ex_units_used.steps ≤ ex_units_budget.steps` for successful scripts, (d) `purpose` matches the redeemer tag in the fixture.

**Checkpoint**: `cargo test -p acropolis_module_utxo_state` passes with the augmented assertions; the rest of the workspace still compiles.

---

## Phase 3: User Story 1 - Live monitoring of phase-2 script evaluations (Priority: P1) 🎯 MVP

**Goal**: when phase-2 publishing is enabled and the visualizer page is open, the operator sees rows appear at the top of the table, one per Plutus script evaluation, in real time.

**Independent Test**: per spec → run the omnibus with `publish-phase2-results = true` on a network with Plutus traffic, open `http://127.0.0.1:8030/`, observe rows arriving (newest at top) within 3 s of each evaluation, table caps at 1000 rows.

### Tests for User Story 1 (write FIRST, confirm fail)

- [X] T011 [P] [US1] Add unit test in `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/stream.rs` (in a `#[cfg(test)] mod tests`) asserting `fan_out(block_info, msg)` emits exactly `msg.outcomes.len()` `ScriptEvalEvent`s, each carrying the correct epoch/block_number/tx_hash/script_hash/purpose/plutus_version/mem/cpu/success per [data-model.md §3](./data-model.md).
- [X] T012 [P] [US1] Add unit test for SSE JSON serialization in the same `mod tests` (or a sibling `mod sse_serde_tests`) asserting `serde_json::to_string(&event)` produces a JSON object whose keys exactly match the `sse-stream.md` contract (`id`, `epoch`, `blockNumber`, `blockHash`, `txHash`, `scriptHash`, `purpose`, `plutusVersion`, `mem`, `cpu`, `memBudget`, `cpuBudget`, `success`, `error` (optional)). Use `#[serde(rename_all = "camelCase")]` on `ScriptEvalEvent` to satisfy this.
- [X] T013 [P] [US1] Add integration test at `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/tests/stream_integration.rs` that spins up the module's broadcast channel + subscription handler in-process, publishes a synthetic `Phase2EvaluationResultsMessage` (with 3 outcomes) onto a fake topic, and asserts the receiver yields exactly 3 `ScriptEvalEvent`s in order with the correct fields. Use `acropolis_common` types; do not depend on the full Caryatid bus.

### Implementation for User Story 1

- [X] T014 [P] [US1] Implement `ScriptEvalEvent` (with `#[serde(rename_all = "camelCase")]`) and `fan_out(block_info: &BlockInfo, msg: &Phase2EvaluationResultsMessage, next_id: &AtomicU64) -> Vec<ScriptEvalEvent>` in `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/stream.rs`. Must satisfy T011 and T012.
- [X] T015 [P] [US1] Implement config parsing in `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/config.rs`: keys `phase2-subscribe-topic` (default `"cardano.utxo.phase2"`), `bind-address` (default `"127.0.0.1"`), `bind-port` (default `8030`). Use the existing `get_string_flag` helper from `acropolis_common::configuration` where applicable.
- [X] T016 [P] [US1] Implement HTTP server scaffold at `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/http.rs`: an `axum::Router` with `GET /` returning the `include_str!("assets/index.html")` body with `Content-Type: text/html; charset=utf-8`, `GET /healthz` returning `200 ok`, and a placeholder `GET /events` that returns `503` until T017 lands. Bind via `tokio::net::TcpListener` + `axum::serve`.
- [X] T017 [US1] Implement `GET /events` SSE handler in `http.rs` per [contracts/sse-stream.md](./contracts/sse-stream.md): accept a `broadcast::Receiver<ScriptEvalEvent>`, wrap with `tokio_stream::wrappers::BroadcastStream`, emit one `event: init` first (fields `cexplorerBaseUrl: ""` and `network: ""` for now — US2 fills these in), then map each successful recv to `axum::response::sse::Event::default().event("script_eval").id(id).data(json)` and each `Err(Lagged(n))` to a single `event: lagged` with `data: {"skipped":n}`. Send a `:heartbeat` comment line every 15 s. On serialization error: `tracing::warn!`, drop that one event.
- [X] T018 [US1] Implement the `#[module(...)]` entry in `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/script_eval_visualizer.rs`: name `"script-eval-visualizer"`, `init` parses config (T015), creates `broadcast::channel::<ScriptEvalEvent>(4096)`, registers a Caryatid subscription on the configured topic that pattern-matches `Message::Cardano((block_info, CardanoMessage::Phase2EvaluationResults(msg)))`, runs `fan_out`, and sends each event via `tx.send(event).ok();`. Spawns the HTTP server (T016/T017) on the `Sender::subscribe()` factory so each new HTTP connection gets a fresh receiver. No `unwrap`/`panic`; errors logged and `?`-propagated.
- [X] T019 [P] [US1] Implement the embedded frontend at `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/assets/index.html`: a single self-contained file with inline `<style>` (basic table styling, success vs failure row colours per FR-007) and inline `<script>` containing: a module-level `state = { rows: [], cexplorerBaseUrl: '', network: '' }`, a `render()` function that rebuilds `<tbody>` from `state.rows`, an `applyEvent(e)` that prepends and truncates to 1000, and an `EventSource('/events')` whose `init` listener writes `state.cexplorerBaseUrl`/`state.network` and whose `script_eval` listener calls `applyEvent`. Columns per FR-006 (Epoch, Block, Tx, Script, Purpose, Plutus, Mem, CPU, Status). Block and Tx cells are plain text in this story (US2 turns them into links). Empty-state message per FR-011: "Waiting for evaluations…".
- [X] T020 [P] [US1] Add publishing config keys to `/home/hypo/works/input-output-hk/acropolis/modules/utxo_state/src/utxo_state.rs`: parse `publish-phase2-results: bool` (default `false`) and `publish-phase2-topic: String` (default `"cardano.utxo.phase2"`); construct a publisher only when the flag is `true`. Pass the publisher (as `Option<...>`) into `run`.
- [X] T021 [US1] Wire the publisher into the validate loop in `/home/hypo/works/input-output-hk/acropolis/modules/utxo_state/src/utxo_state.rs::run` (or `state.rs` per existing flow): after `state.validate(...)` returns its outcomes vec, iterate it and, **only if** the publisher is `Some`, build one `Phase2EvaluationResultsMessage` per non-empty tx and publish on the configured topic wrapped in `Message::Cardano((block_info.clone(), CardanoMessage::Phase2EvaluationResults(msg)))`. The `Some`/`None` check MUST be the first thing inspected, before any allocation, so the off-path is single-cmp (this is the SC-005 zero-cost requirement).
- [X] T022 [P] [US1] Add `acropolis_module_script_eval_visualizer = { path = "../../modules/script_eval_visualizer" }` to `/home/hypo/works/input-output-hk/acropolis/processes/omnibus/Cargo.toml` (alphabetical near `mcp_server`).
- [X] T023 [US1] Register the module in `/home/hypo/works/input-output-hk/acropolis/processes/omnibus/src/main.rs` (`ScriptEvalVisualizer::register(&mut process);` next to other module registrations) and add `[module.script-eval-visualizer]` section to `/home/hypo/works/input-output-hk/acropolis/processes/omnibus/omnibus.toml` with the three default config keys (commented or uncommented per project convention) plus `publish-phase2-results = true` under `[module.utxo-state]` for *local-dev* use (the production default in code stays `false`).

**Checkpoint**: with `publish-phase2-results = true`, `make run` shows the visualizer at `http://127.0.0.1:8030/`, an `EventSource` connection in dev tools, and rows appearing as Plutus blocks are processed. Tests T010–T013 pass.

---

## Phase 4: User Story 2 - Drill into a transaction or block via cexplorer.io (Priority: P2)

**Goal**: clicking the block-number or transaction-hash cell opens a new tab to the corresponding cexplorer.io page on the node's network.

**Independent Test**: at least one row visible → click block cell → new tab opens to the right cexplorer.io URL → click tx cell → new tab opens to the right cexplorer.io URL → visualizer tab keeps streaming.

### Tests for User Story 2

- [X] T024 [P] [US2] Add unit test in `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/script_eval_visualizer.rs` (`#[cfg(test)] mod cexplorer_url_tests`) asserting the network → base URL mapping per [research.md Q5](./research.md): mainnet → `https://cexplorer.io`, preprod → `https://preprod.cexplorer.io`, preview → `https://preview.cexplorer.io`, sancho/other → `https://cexplorer.io` (best-effort fallback).
- [X] T025 [P] [US2] Add integration test in `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/tests/stream_integration.rs` (extend the file from T013) asserting that the first event a fresh SSE client receives is the `init` event with the correct `cexplorerBaseUrl` and `network` for the configured network.

### Implementation for User Story 2

- [X] T026 [P] [US2] Add `network` config key to `modules/script_eval_visualizer/src/config.rs` (default `"mainnet"`) and a `cexplorer_base_url(network: &str) -> &'static str` helper used by both the SSE handler and tests. Plumb network into the module state via `init`.
- [X] T027 [US2] Update the SSE `init` event in `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/http.rs` to emit `{"cexplorerBaseUrl": "...", "network": "..."}` populated from the values plumbed in T026, replacing the empty placeholders left in T017.
- [X] T028 [US2] Update `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/src/assets/index.html` so the block cell renders as `<a href="${cexplorerBaseUrl}/block/${blockHash}" target="_blank" rel="noopener noreferrer">${blockNumber}</a>` and the tx cell renders as `<a href="${cexplorerBaseUrl}/tx/${txHash}" target="_blank" rel="noopener noreferrer">${shortTxHash}</a>`. Use the values stored from the `init` event.

**Checkpoint**: all US1 tests still pass. Manual click-through on a running node opens the right cexplorer pages in new tabs.

---

## Phase 5: User Story 3 - Toggle phase-2 result publishing on and off (Priority: P2)

**Goal**: operator turns publishing off and the node performs no work for this feature; turns it on and rows resume.

**Independent Test**: with the visualizer open, set `publish-phase2-results = false`, restart, observe no new rows; set back to `true`, restart, observe rows resume.

### Tests for User Story 3

- [X] T029 [P] [US3] Add unit test in `/home/hypo/works/input-output-hk/acropolis/modules/utxo_state/src/utxo_state.rs` (or a `#[cfg(test)]` sibling) asserting that when the publisher is `None` (flag off), the per-tx publish loop in T021 short-circuits **before** constructing any `Phase2EvaluationResultsMessage` or cloning any `BlockInfo`. Use a counter or `tracing-test` capture to assert no allocations beyond the single bool check.
- [X] T030 [P] [US3] Add a regression test asserting `evaluate_scripts` is called the same number of times whether the flag is on or off (i.e., publishing is purely additive — disabling it MUST NOT change validation behaviour).

### Implementation for User Story 3

- [X] T031 [US3] Confirm the production default in `/home/hypo/works/input-output-hk/acropolis/processes/omnibus/omnibus.toml` is `publish-phase2-results = false` under `[module.utxo-state]` (T023 left it `true` for local-dev — change to `false` here for the spec-correct default; keep a commented `# publish-phase2-results = true` line as a hint to operators).
- [X] T032 [US3] Document the toggle and its zero-cost guarantee in `/home/hypo/works/input-output-hk/acropolis/modules/utxo_state/README.md` under a new "Phase-2 evaluation publishing" section. Keep it short — a paragraph + the two config keys.
- [X] T033 [US3] Cross-reference the toggle in a fresh `/home/hypo/works/input-output-hk/acropolis/modules/script_eval_visualizer/README.md` (one-pager: what the module does, the two endpoints, the dependency on `utxo_state`'s flag).

**Checkpoint**: all earlier tests pass; toggling the flag in `omnibus.toml` produces the documented behaviour end-to-end.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T034 [P] Doc comments (`///`) on every public item in `modules/script_eval_visualizer/src/*.rs` and on the new common types per constitution rule 4. Audit with `cargo doc -p acropolis_module_script_eval_visualizer --no-deps` — no missing-doc warnings. *(Audit clean for our items; the only `-W missing-docs` hit is on the `register` associated function emitted by `caryatid_sdk`'s `#[module(...)]` proc-macro and is out of scope for this module — every other module in-tree shares the same situation.)*
- [X] T035 [P] Run `make fmt` and `make clippy` and resolve any warnings (constitution rule 3 — `-D warnings`). *(Reformatted `processes/omnibus/src/main.rs` import order; fixed two `clippy::clone_on_copy` warnings on `ExUnits` in `modules/utxo_state/src/validations/phase_two/evaluate.rs`. `cargo clippy --workspace --all-targets -- -D warnings` is clean.)*
- [X] T036 Run `make test` and confirm all unit + integration tests pass. *(All workspace tests pass except the pre-existing flaky `test_calibration_stability` integration test in `modules/utxo_state/tests/calibration.rs`, which is environment/timing-sensitive (CV threshold 25%) and reproducibly fails on baseline `main` without our changes too — explicitly out of scope.)*
- [ ] T037 Manually run the [quickstart.md](./quickstart.md) procedure end-to-end on a network with Plutus traffic; tick each acceptance check (FR-006/SC-003, FR-008/SC-004, FR-009, FR-010/SC-002, FR-007, FR-002/SC-005). Record any deviations as follow-up issues. *(Deferred — requires running the omnibus on a live network with Plutus traffic, which is operator-local manual verification. Smoke-test command set from the quickstart §"Smoke test (CI / dev loop)" all pass: `cargo test -p acropolis_module_script_eval_visualizer`, `cargo test --test stream_integration -p acropolis_module_script_eval_visualizer`, `cargo build -p acropolis_process_omnibus`. The browser-side acceptance checks remain to be ticked off manually.)*

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: no dependencies — start immediately.
- **Foundational (Phase 2)**: depends on Phase 1; **blocks Phases 3–5**.
- **US1 (Phase 3)**: depends on Phase 2. Independently demonstrable once T010–T023 are done.
- **US2 (Phase 4)**: depends on US1 being functional (it extends the same module + frontend file). Independently testable after T024–T028.
- **US3 (Phase 5)**: depends on US1's publishing path existing (T020–T021); test/verification-heavy. Can run in parallel with US2 (different files).
- **Polish (Phase 6)**: depends on all desired stories being complete.

### Within each user story

- Tests (Txx, Txy, Txz) MUST be written and FAIL before the implementation tasks that satisfy them.
- Within US1: T020/T021 (publisher) and T014–T019 (visualizer + frontend) touch different files and are mostly parallelizable, but T021 follows T009 (state.rs return shape) and T018 follows T014/T015/T016/T017 (it wires the pieces together).

### Parallel Opportunities

- **T003, T004** in Setup.
- **T005, T006, T010** in Foundational (T010 only after T007 lands the new return shape — sequence inside the phase).
- **T011, T012, T013** test-writing in US1 — all parallel.
- **T014, T015, T016, T019, T020, T022** implementation in US1 — all touch distinct files.
- **T024, T025, T026** in US2.
- **T029, T030** in US3.
- US2 and US3 can be developed in parallel after US1 is green.

---

## Parallel Example: User Story 1

```bash
# All three test tasks (different files / different mod tests blocks):
Task: "T011 [P] [US1] fan_out unit test in modules/script_eval_visualizer/src/stream.rs"
Task: "T012 [P] [US1] SSE serde test in modules/script_eval_visualizer/src/stream.rs (sibling mod)"
Task: "T013 [P] [US1] In-process integration test in modules/script_eval_visualizer/tests/stream_integration.rs"

# Implementation work that touches distinct files (run after Phase 2):
Task: "T014 [P] [US1] ScriptEvalEvent + fan_out in modules/script_eval_visualizer/src/stream.rs"
Task: "T015 [P] [US1] Config parsing in modules/script_eval_visualizer/src/config.rs"
Task: "T016 [P] [US1] axum router scaffold in modules/script_eval_visualizer/src/http.rs"
Task: "T019 [P] [US1] Frontend at modules/script_eval_visualizer/src/assets/index.html"
Task: "T020 [P] [US1] Publishing config in modules/utxo_state/src/utxo_state.rs"
Task: "T022 [P] [US1] Add dep to processes/omnibus/Cargo.toml"
```

---

## Implementation Strategy

### MVP First (User Story 1 only)

1. Phase 1: Setup (T001–T004).
2. Phase 2: Foundational (T005–T010). **Critical** — blocks everything.
3. Phase 3: US1 (T011–T023). Write tests T011–T013 first; implement T014–T023; verify tests green.
4. **STOP and validate**: run the omnibus with `publish-phase2-results = true`, browse `http://127.0.0.1:8030/`, confirm rows arrive.
5. Demo / merge as MVP.

### Incremental Delivery

- After MVP: layer US2 (cexplorer links — T024–T028) and US3 (toggle verification + docs — T029–T033) in any order; both are additive and do not regress US1.
- Final: Phase 6 polish (T034–T037).

### Parallel Team Strategy

- **Developer A** completes Phase 1 + Phase 2 alone (most are single-file edits in `common/` and `utxo_state/` and need to merge in order).
- After Phase 2:
  - **Developer A**: US1 backend (T014–T018, T020–T023).
  - **Developer B**: US1 frontend (T019) + US2 (T024–T028) once T017 merges.
  - **Developer C**: US3 verification (T029–T033) once T020–T021 merge.

---

## Notes

- `[P]` = different file, no upstream dependency on an incomplete task.
- Per L008: the doc comment on `evaluate_scripts` must be updated when its return type changes (T007).
- Per L001: rollback semantics are deliberately *not* "rescind on rollback" — covered in `Phase2EvaluationResultsMessage`'s contract; do not add rescind logic later without revisiting the spec.
- Constitution rule 3: every new code path uses `Result`/`?`. Audit during T035.
- Avoid: cross-story coupling that would break US1 if US2/US3 were skipped. The current task list keeps US2 strictly additive (links + init payload) and US3 strictly verification + default-flag-flip + docs.
