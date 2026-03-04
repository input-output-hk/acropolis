# Feature Specification: UPLC Performance Test Normalization

**Feature Branch**: `002-uplc-perf-normalization`  
**Created**: 2026-02-26  
**Status**: Draft  
**Input**: User description: "Add normalization feature for UPLC performance tests to use calibrated runtime baselines instead of fixed time thresholds"

## User Scenarios & Testing

### Story 1 - Machine-Adaptive Benchmark Thresholds (P1)

As a developer, I want UPLC benchmark tests to replace hardcoded 100ms limits with machine-calibrated thresholds, so tests pass on slow CI runners without masking regressions on fast machines.

**Acceptance Scenarios**:

1. **Given** a slow/loaded machine, **When** benchmarks run, **Then** thresholds scale proportionally to the calibration baseline — no false failures.
2. **Given** a fast machine, **When** benchmarks run, **Then** thresholds stay tight and genuine regressions (≥3x slowdown) are caught.
3. **Given** the calibration function runs repeatedly, **Then** results are consistent (CV < 15%).

### Story 2 - Enhanced Diagnostic Output (P2)

As a developer investigating a benchmark result, I want test output to show the calibration baseline, computed threshold, raw time, and normalized ratio, so I can understand pass/fail without extra investigation.

**Acceptance Scenarios**:

1. **Given** any benchmark test completes, **Then** output shows raw time, baseline, threshold, and ratio (e.g., "2.5x baseline").
2. **Given** the bulk test (`test_all_benchmark_scripts`), **Then** the summary table includes normalized columns and reports the baseline/multiplier used.

### Edge Cases

- Calibration takes unexpectedly long (frozen machine) → still produces a valid high baseline, no panic.
- Near-zero baseline (very fast machine) → minimum absolute floor prevents false failures from rounding noise.
- Inconsistent calibration results → multiple iterations with median statistic absorb outliers.
- Different CPU architecture (ARM vs x86) → calibration measures wall-clock time, works transparently.

## Requirements

### Functional Requirements

- **FR-001**: Provide a calibration function that evaluates a small, bundled UPLC program through the existing evaluator pool, returning a baseline duration (ms). This ensures the baseline measures the same code path (thread pool dispatch, FLAT decoding, UPLC evaluation) that the benchmarks use.
- **FR-002**: Calibration MUST run multiple iterations and return the median to mitigate scheduling outliers. Total calibration time MUST NOT exceed 200ms.
- **FR-003**: Performance thresholds expressed as `multiplier × baseline` (default 5x), with a minimum absolute floor.
- **FR-004**: Update all existing benchmark tests (auction, uniswap, stablecoin, bulk) to use calibrated thresholds — remove all hardcoded 100ms/50ms limits.
- **FR-005**: Update or supplement `RawEvalResult::within_target` to accept a calibrated threshold.
- **FR-006**: Test output MUST include baseline, threshold, raw time, and normalized ratio.
- **FR-007**: Calibration runs once per test session (cached) so all tests share the same baseline.

### Key Entities

- **CalibrationBaseline**: Median baseline duration + iteration count + variance stats for the current machine.
- **NormalizedThreshold**: Multiplier × baseline with optional absolute minimum floor.

## Success Criteria

- **SC-001**: Zero false failures from machine variability on CI runners that previously had intermittent failures.
- **SC-002**: Calibration CV < 15% across 10 consecutive runs under stable conditions.
- **SC-003**: Performance regressions ≥3x are still detected on slow machines.
- **SC-004**: No hardcoded absolute time limits remain in test assertions.
- **SC-005**: Diagnostic output sufficient for a developer to understand pass/fail without extra investigation.

## Assumptions

- The calibration workload will evaluate a small, bundled UPLC program through the same evaluator pool used by benchmarks, ensuring maximal correlation with real script evaluation.
- A median of 5-10 calibration iterations provides sufficient stability for baseline measurement, within a total budget of ≤200ms.
- The default multiplier is 5x baseline. On a fast machine (~2ms baseline) the threshold is ~10ms; on a loaded CI runner (~20ms baseline) it becomes ~100ms, matching the original intent while still catching ≥3x regressions.
- The absolute minimum floor for thresholds will be set around 10ms to handle extremely fast machines gracefully.
- The calibration result will be cached using a lazy-static or once-cell pattern so that the cost is paid only once per test binary execution.
- The calibration workload should take roughly 1-5ms on a modern developer machine, similar in character to the UPLC evaluation it's normalizing against.

## Scope Boundaries

### In Scope

- Calibration function and infrastructure
- Updating existing benchmark tests to use normalized thresholds
- Diagnostic output improvements
- Caching of calibration results within a test session

### Out of Scope

- Historical trend tracking or performance regression dashboards
- Persisting calibration results across test runs
- Modifying the actual UPLC evaluator performance
- Changes to production (non-test) code beyond the `within_target` method update
- CI pipeline configuration changes (the normalization should work transparently)

## Clarifications

### Session 2026-02-26

- Q: What should the calibration workload be? → A: Evaluate a small bundled UPLC program via the existing evaluator pool (Option A).
- Q: What default multiplier for thresholds? → A: 5x baseline — balanced default accommodating normal CI variance.
- Q: How much calibration overhead per test session? → A: ≤200ms total budget — comfortable for ~10 iterations, imperceptible delay.
