# Tasks: UPLC Performance Test Normalization

**Input**: Design documents from `/specs/002-uplc-perf-normalization/`
**Prerequisites**: plan.md, spec.md

## Format: `[ID] [P?] [Story?] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story (US1 = Machine-Adaptive Thresholds, US2 = Enhanced Diagnostic Output)
- Exact file paths included in descriptions

---

## Phase 1: Trial Runs — Select Calibration Script

**Purpose**: Find the best .flat script for calibration (~5ms on laptop, ~25ms on CI)

- [x] T001 Write temporary timing harness `test_calibration_candidates` in modules/tx_unpacker/tests/phase2_test.rs that runs each .flat script 10 times and prints median/mean/stddev table
- [x] T002 Run timing harness locally (`cargo test -p acropolis_module_tx_unpacker -- test_calibration_candidates --nocapture`) and record results for prism-1, auction_1-1, token-account-1, crowdfunding-success-1
- [x] T003 Select the script closest to ~5ms median with lowest CV; document selection in a code comment in modules/tx_unpacker/tests/calibration.rs

**Checkpoint**: ✅ One calibration script selected — `escrow-redeem_1-1.flat` (5.6KB, ~5.1ms median, 3.1% CV)

---

## Phase 2: Foundational — Calibration Infrastructure

**Purpose**: Core calibration types, function, and caching — MUST complete before user story work

**⚠️ CRITICAL**: No benchmark test updates can begin until this phase is complete

- [x] T004 [P] Create CalibrationBaseline struct with `median_ms`, `iteration_count`, `cv_percent` fields and `threshold_ms(multiplier, floor)` method in modules/tx_unpacker/tests/common/mod.rs
- [x] T005 [P] Add `within_calibrated_target(&self, threshold_ms: f64) -> bool` method to RawEvalResult in modules/tx_unpacker/src/validations/phase2.rs
- [x] T006 Implement `calibrate(iterations: usize) -> CalibrationBaseline` function using `include_bytes!` for the selected script in modules/tx_unpacker/tests/common/mod.rs
- [x] T007 Implement `get_calibration() -> &'static CalibrationBaseline` with OnceLock caching (10 iterations, default 10x multiplier, 10ms floor) in modules/tx_unpacker/tests/common/mod.rs
- [x] T008 Add `test_calibration_stability` test: run calibrate(10) five times, assert CV < 20% each, assert medians within 30% of each other in modules/tx_unpacker/tests/calibration.rs
- [x] T009 Verify calibration compiles and stability test passes: `cargo test -p acropolis_module_tx_unpacker -- test_calibration_stability --nocapture`

**Checkpoint**: ✅ `get_calibration()` returns stable cached baseline; shared via `tests/common/mod.rs`; `within_calibrated_target` available on RawEvalResult

---

## Phase 3: User Story 1 — Machine-Adaptive Benchmark Thresholds (P1) 🎯 MVP

**Goal**: Replace all hardcoded 100ms/50ms limits with `10× calibration baseline` (floor 10ms)

**Independent Test**: `cargo test -p acropolis_module_tx_unpacker -- benchmark --nocapture` — all benchmarks pass using calibrated thresholds

- [x] T010 [US1] Update `test_eval_benchmark_auction` to use `get_calibration().default_threshold_ms()` instead of hardcoded 100.0 in modules/tx_unpacker/tests/phase2_test.rs
- [x] T011 [US1] Update `test_eval_benchmark_uniswap` to use calibrated threshold instead of hardcoded 100.0 in modules/tx_unpacker/tests/phase2_test.rs
- [x] T012 [US1] Update `test_eval_benchmark_stablecoin` to use calibrated threshold instead of hardcoded 100.0 in modules/tx_unpacker/tests/phase2_test.rs
- [x] T013 [US1] Update `test_all_benchmark_scripts` to use calibrated threshold instead of hardcoded 100.0 and 50.0 in modules/tx_unpacker/tests/phase2_test.rs
- [x] T014 [US1] Replace ALL hardcoded timing assertions (SC-001 tests too: `test_sc001_eval_performance_p95`, `test_sc001_spending_validator_performance`, `test_sc001_large_script_performance`, `test_sc001_parallel_performance`) with calibrated thresholds
- [x] T015 [US1] Run all benchmark tests to verify they pass with calibrated thresholds: `cargo test -p acropolis_module_tx_unpacker`

**Checkpoint**: ✅ US1 complete — zero hardcoded time limits remain; all benchmarks use calibrated thresholds

---

## Phase 4: User Story 2 — Enhanced Diagnostic Output (P2)

**Goal**: Each benchmark prints baseline, threshold, raw time, and normalized ratio

**Independent Test**: Run any benchmark test with `--nocapture` and verify output contains "Baseline:", "Threshold:", and "Ratio:" lines

- [x] T016 [US2] Add diagnostic print line to each individual benchmark test (auction, uniswap, stablecoin) showing `Baseline: {:.3}ms | Threshold: {:.3}ms (10.0x) | Elapsed: {:.3}ms | Ratio: {:.1}x` in modules/tx_unpacker/tests/phase2_test.rs
- [x] T017 [US2] Add calibration header and "Ratio" column to the results table in `test_all_benchmark_scripts` in modules/tx_unpacker/tests/phase2_test.rs
- [x] T018 [US2] Update summary section in `test_all_benchmark_scripts` to report baseline and multiplier used in modules/tx_unpacker/tests/phase2_test.rs
- [x] T019 [US2] Run benchmark tests with `--nocapture` and verify diagnostic output is present and readable

**Checkpoint**: ✅ US2 complete — all test output includes calibration diagnostics

---

## Phase 5: Polish & Validation

**Purpose**: Final cleanup, load testing, and quality gate

- [x] T020 Mark `test_calibration_candidates` as `#[ignore]` in modules/tx_unpacker/tests/phase2_test.rs
- [x] T021 Update code comments referencing "SC-001: <100ms" to reference calibrated targets in modules/tx_unpacker/src/validations/phase2.rs and modules/tx_unpacker/tests/phase2_test.rs
- [x] T022 Run full test suite: `cargo test -p acropolis_module_tx_unpacker` — 28 passed, 1 ignored, 0 failed
- [x] T023 Run `cargo clippy --tests -D warnings` and `cargo fmt --check` — clean
- [x] T024 Run benchmarks under simulated load (`nice -n 19`) — all pass, calibration adapts correctly

**Checkpoint**: ✅ All tests pass under normal and loaded conditions; clippy + fmt clean

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Trial Runs)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 (script selection) — BLOCKS all user stories
- **Phase 3 (US1)**: Depends on Phase 2 completion
- **Phase 4 (US2)**: Depends on Phase 2 completion; can run in parallel with US1 but touches same file
- **Phase 5 (Polish)**: Depends on Phase 3 + Phase 4 completion

### Within Each Phase

- T004 and T005 can run in parallel (different files)
- T010–T013 touch the same file — execute sequentially or as a single editing pass
- T016–T018 touch the same file — execute sequentially or as a single editing pass

### Recommended Execution Order

T001 → T002 → T003 → (T004 ∥ T005) → T006 → T007 → T008 → T009 → T010–T015 → T016–T019 → T020–T024

---

## Implementation Strategy

### MVP First (Story 1 Only)

1. Phase 1: Select calibration script (T001–T003)
2. Phase 2: Build infrastructure (T004–T009)
3. Phase 3: Replace hardcoded thresholds (T010–T015)
4. **STOP and VALIDATE**: All benchmarks pass with calibrated thresholds
5. This alone delivers SC-001 (zero false CI failures)

### Full Delivery

6. Phase 4: Add diagnostic output (T016–T019)
7. Phase 5: Polish and validate (T020–T024)

---

## Notes

- All changes are in 2 files (phase2_test.rs, phase2.rs) plus 2 new files (calibration.rs, common/mod.rs)
- Calibration infrastructure shared via `tests/common/mod.rs` (standard Rust integration test pattern)
- No new crate dependencies required — uses `std::sync::OnceLock` and existing `evaluate_raw_flat_program`
- Multiplier bumped from 5x to 10x: uniswap-3 runs at 4.5–5.3x baseline locally but ~6.25x on CI; 10x provides generous CI headroom
- Commit after each phase checkpoint for clean rollback points
