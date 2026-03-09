# Implementation Plan: UPLC Performance Test Normalization

**Branch**: `002-uplc-perf-normalization` | **Date**: 2026-02-26 | **Spec**: [spec.md](spec.md)

## Summary

Replace hardcoded 100ms/50ms timing thresholds in UPLC benchmark tests with machine-calibrated baselines. A calibration function evaluates a small bundled UPLC program through the existing evaluator pool, caches the result, and all benchmark assertions use `multiplier × baseline` (default 5x) with a minimum floor.

## Technical Context

**Language/Version**: Rust (workspace edition 2021)  
**Primary Dependencies**: `uplc-turbo` (UPLC evaluation), `rayon` (thread pool), `std::sync::OnceLock` (caching)  
**Testing**: `cargo test -p acropolis_module_tx_unpacker`  
**Target Platform**: Linux CI (GitHub Actions) + macOS developer laptops  
**Performance Goals**: Calibration ≤200ms total; benchmark script thresholds = 5× calibration baseline  
**Constraints**: No new crate dependencies; calibration must use the existing evaluator pool code path  

## Project Structure

### Source Code Changes

```text
modules/tx_unpacker/
├── src/validations/phase2.rs         # Update RawEvalResult::within_target, add within_calibrated_target
└── tests/
    ├── phase2_test.rs                # Update all benchmark tests to use calibrated thresholds
    └── calibration.rs                # NEW: calibration module (function, caching, types)

tests/fixtures/plutus_scripts/
└── (existing scripts)                # Trial runs to select the best calibration candidate
```

## Implementation Phases

### Phase 0 — Trial Runs: Select Calibration Script

**Goal**: Identify which existing .flat script (or subset thereof) runs ~5ms on a developer laptop (~25ms on CI), to serve as the calibration workload.

**Context**: uniswap-3.flat runs ~20ms locally / ~125ms on CI (ratio ~6.25x). We need something ~4x smaller in evaluation time. The smaller scripts (prism-1 at 3KB, auction_1-1 at 3.7KB) are good candidates.

**Steps**:

1. **Write a standalone timing harness** in the test file that runs each .flat script 10 times, reports median/mean/stddev for each. This is a temporary test function (`test_calibration_candidates`) that prints a comparison table.

2. **Run locally** — identify scripts with median ~5ms. Candidates ordered by likelihood:
   - `prism-1.flat` (3.0KB — smallest)
   - `auction_1-1.flat` (3.7KB)
   - `token-account-1.flat` (4.0KB)
   - `crowdfunding-success-1.flat` (4.5KB)

3. **Run on CI** (or simulate load with `stress-ng`) — verify the selected script hits ~25ms under load, giving a laptop:CI ratio of ~5x consistent with uniswap's ~6.25x ratio.

4. **Select the winner** — pick the script whose median is closest to 5ms locally with the lowest CV (coefficient of variation). If none of the existing scripts land cleanly at 5ms, consider using the best candidate as-is (the exact value doesn't matter as long as it's stable — the multiplier adjusts).

5. **Remove the temporary timing harness** after selection is made, or keep it as a `#[ignore]` test for future re-calibration.

**Exit Criteria**: One script selected, documented in code comment, with measured timings on at least one machine.

---

### Phase 1 — Calibration Infrastructure

**Goal**: Build the calibration function, types, and caching mechanism.

**Files**: `modules/tx_unpacker/tests/calibration.rs` (new), `modules/tx_unpacker/src/validations/phase2.rs` (minor update)

#### Task 1.1 — CalibrationBaseline type

```rust
/// Result of machine calibration.
pub struct CalibrationBaseline {
    pub median_ms: f64,
    pub iteration_count: usize,
    pub cv_percent: f64,  // coefficient of variation
}

impl CalibrationBaseline {
    /// Compute a normalized threshold: max(multiplier * baseline, floor_ms)
    pub fn threshold_ms(&self, multiplier: f64, floor_ms: f64) -> f64 {
        (self.median_ms * multiplier).max(floor_ms)
    }
}
```

#### Task 1.2 — Calibration function

```rust
/// Run the calibration workload and return the baseline.
/// Evaluates the bundled calibration script N times, returns median.
pub fn calibrate(iterations: usize) -> CalibrationBaseline {
    let script_bytes = include_bytes!("../../../tests/fixtures/plutus_scripts/<selected>.flat");
    let mut timings: Vec<f64> = Vec::with_capacity(iterations);
    
    for _ in 0..iterations {
        let result = evaluate_raw_flat_program(script_bytes)
            .expect("Calibration script must evaluate successfully");
        timings.push(result.elapsed_ms());
    }
    
    timings.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_ms = timings[timings.len() / 2];
    // ... compute CV ...
    
    CalibrationBaseline { median_ms, iteration_count: iterations, cv_percent }
}
```

#### Task 1.3 — Session caching with OnceLock

```rust
use std::sync::OnceLock;

static CALIBRATION: OnceLock<CalibrationBaseline> = OnceLock::new();

/// Get or compute the calibration baseline (cached per process).
pub fn get_calibration() -> &'static CalibrationBaseline {
    CALIBRATION.get_or_init(|| calibrate(10))
}
```

#### Task 1.4 — Update RawEvalResult

Add a method to `RawEvalResult` that accepts a calibrated threshold:

```rust
impl RawEvalResult {
    /// Check if evaluation completed within a calibrated target.
    pub fn within_calibrated_target(&self, threshold_ms: f64) -> bool {
        self.elapsed_ms() < threshold_ms
    }
}
```

**Exit Criteria**: `calibrate()` returns stable results (CV < 15%), `get_calibration()` caches correctly, `within_calibrated_target` compiles. Unit test for calibration stability passes.

---

### Phase 2 — Update Benchmark Tests

**Goal**: Replace all hardcoded thresholds with calibrated assertions and enhanced output.

**File**: `modules/tx_unpacker/tests/phase2_test.rs`

#### Task 2.1 — Update individual benchmark tests

For each of `test_eval_benchmark_auction`, `test_eval_benchmark_uniswap`, `test_eval_benchmark_stablecoin`:

- Call `get_calibration()` at test start
- Compute threshold via `baseline.threshold_ms(5.0, 10.0)`
- Replace `assert!(elapsed_ms < 100.0, ...)` with `assert!(elapsed_ms < threshold, ...)`
- Print diagnostic line: `"Baseline: {:.3}ms | Threshold: {:.3}ms (5.0x) | Elapsed: {:.3}ms | Ratio: {:.1}x"`

#### Task 2.2 — Update bulk benchmark test

For `test_all_benchmark_scripts`:

- Add calibration info to the header output
- Add "Ratio" column to the results table
- Replace the `< 100.0` and `< 50.0` assertions with calibrated threshold
- Update summary to show baseline and multiplier used

#### Task 2.3 — Calibration stability test

Add `test_calibration_stability`:
- Run `calibrate(10)` 5 times
- Assert CV < 15% for each run
- Assert the 5 median values are within 20% of each other

**Exit Criteria**: `cargo test -p acropolis_module_tx_unpacker -- benchmark` passes with all calibrated thresholds. No hardcoded `100.0` or `50.0` time limits remain. Test output shows baseline/threshold/ratio.

---

### Phase 3 — Validation & Cleanup

**Goal**: Final verification and documentation.

#### Task 3.1 — Run full test suite

```bash
cargo test -p acropolis_module_tx_unpacker
```

Ensure no regressions in existing non-benchmark tests.

#### Task 3.2 — Simulate load test

Run benchmarks under `stress-ng` or `nice -n 19` to verify that calibrated thresholds adapt correctly and tests still pass under load.

#### Task 3.3 — Clean up

- Remove any temporary trial-run test code (or mark `#[ignore]`)
- Update code comments referencing "SC-001: <100ms" to reference calibrated targets
- Ensure `clippy` and `fmt` pass: `make clippy && make fmt`

**Exit Criteria**: All tests pass under both normal and loaded conditions. `make all` succeeds.

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| No existing script hits ~5ms target | Use whatever's closest; the multiplier compensates. Exact calibration time is less important than stability. |
| Calibration adds noticeable overhead | OnceLock ensures it runs once. 10 iterations × ~5ms = ~50ms total — well within 200ms budget. |
| CV > 15% on noisy CI machines | Use median (robust to outliers). If still high, increase iterations or add warmup round. |
| `include_bytes!` path breaks | Use relative path from test file; add compile-time assertion test. |
