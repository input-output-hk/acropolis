//! Shared test infrastructure for UPLC performance test normalization.
//!
//! Provides a machine-adaptive calibration baseline for benchmark thresholds,
//! replacing hardcoded absolute time limits. The calibration evaluates a small
//! UPLC program through the same evaluator pool used by benchmarks, ensuring
//! the baseline correlates with actual script evaluation performance.
//!
//! # Calibration Script Selection (Phase 0 results)
//!
//! Script selected: `escrow-redeem_1-1.flat` (5.6KB)
//!
//! Measured on developer laptop (2026-02-26):
//! ```text
//! Script                    Size    Median     Mean    StdDev   CV%
//! token-account-1           3.9KB   2.778ms   2.793ms  0.062ms  2.2%
//! crowdfunding-success-1    4.4KB   3.032ms   3.064ms  0.126ms  4.1%
//! vesting-1                 5.4KB   4.848ms   4.931ms  0.218ms  4.4%
//! escrow-redeem_1-1  ★      5.6KB   5.110ms   5.139ms  0.158ms  3.1%
//! game-sm-success_1-1       8.4KB   5.528ms   5.561ms  0.098ms  1.8%
//! ```
//!
//! Selection rationale:
//! - Median ~5.1ms (closest to 5ms target)
//! - CV 3.1% (excellent stability)
//! - Expected ~32ms on CI (based on ~6.25x laptop:CI ratio from uniswap)
//! - 10 iterations × ~5ms = ~50ms total calibration (well within 200ms budget)

use std::sync::OnceLock;

use acropolis_module_tx_unpacker::validations::phase2::evaluate_raw_flat_program;

/// Default number of calibration iterations.
pub const CALIBRATION_ITERATIONS: usize = 10;

/// Default performance multiplier (threshold = multiplier × baseline).
/// Set to 10x based on empirical data: the largest benchmark script (uniswap-3)
/// runs at ~4.5-5.3x the calibration baseline locally, but can reach ~6.25x on
/// CI under load. 10x gives generous headroom for CI variance while still
/// catching genuine performance regressions.
pub const DEFAULT_MULTIPLIER: f64 = 10.0;

/// Minimum absolute floor for thresholds (ms) to prevent near-zero
/// baselines from causing false failures on very fast machines.
pub const DEFAULT_FLOOR_MS: f64 = 10.0;

/// The calibration script bytes, embedded at compile time.
/// Uses escrow-redeem_1-1.flat selected in Phase 0.
pub const CALIBRATION_SCRIPT: &[u8] =
    include_bytes!("../../../../tests/fixtures/plutus_scripts/escrow-redeem_1-1.flat");

// =============================================================================
// CalibrationBaseline
// =============================================================================

/// Result of machine calibration — represents how fast this machine evaluates
/// a known UPLC workload.
#[derive(Debug, Clone)]
pub struct CalibrationBaseline {
    /// Median evaluation time in milliseconds.
    pub median_ms: f64,
    /// Number of iterations used to compute the baseline.
    pub iteration_count: usize,
    /// Coefficient of variation (stddev/mean × 100) as a percentage.
    pub cv_percent: f64,
}

impl CalibrationBaseline {
    /// Compute a normalized performance threshold.
    ///
    /// Returns `max(multiplier × baseline, floor_ms)` so that:
    /// - On slow machines, the threshold scales up proportionally
    /// - On very fast machines, the floor prevents false failures from noise
    pub fn threshold_ms(&self, multiplier: f64, floor_ms: f64) -> f64 {
        (self.median_ms * multiplier).max(floor_ms)
    }

    /// Compute threshold using the default multiplier and floor.
    pub fn default_threshold_ms(&self) -> f64 {
        self.threshold_ms(DEFAULT_MULTIPLIER, DEFAULT_FLOOR_MS)
    }

    /// Compute the normalized ratio of an elapsed time relative to baseline.
    pub fn ratio(&self, elapsed_ms: f64) -> f64 {
        if self.median_ms > 0.0 {
            elapsed_ms / self.median_ms
        } else {
            f64::INFINITY
        }
    }
}

// =============================================================================
// Calibration function
// =============================================================================

/// Run the calibration workload and return the baseline.
///
/// Evaluates the bundled calibration script (escrow-redeem_1-1.flat) for
/// `iterations` rounds, computes the median timing, and returns a
/// `CalibrationBaseline` with stability statistics.
///
/// A warmup run is performed first and discarded to avoid cold-start effects.
pub fn calibrate(iterations: usize) -> CalibrationBaseline {
    assert!(
        iterations >= 3,
        "Need at least 3 iterations for a meaningful median"
    );

    // Warmup run (discarded)
    evaluate_raw_flat_program(CALIBRATION_SCRIPT).expect("Calibration script warmup must succeed");

    // Timed iterations
    let mut timings: Vec<f64> = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let result = evaluate_raw_flat_program(CALIBRATION_SCRIPT)
            .expect("Calibration script must evaluate successfully");
        timings.push(result.elapsed_ms());
    }

    // Sort for median
    timings.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_ms = timings[timings.len() / 2];

    // Compute mean and CV
    let mean = timings.iter().sum::<f64>() / timings.len() as f64;
    let variance = timings.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / timings.len() as f64;
    let stddev = variance.sqrt();
    let cv_percent = if mean > 0.0 {
        (stddev / mean) * 100.0
    } else {
        0.0
    };

    CalibrationBaseline {
        median_ms,
        iteration_count: iterations,
        cv_percent,
    }
}

// =============================================================================
// Session caching
// =============================================================================

/// Cached calibration baseline — computed once per test binary execution.
static CALIBRATION: OnceLock<CalibrationBaseline> = OnceLock::new();

/// Get or compute the calibration baseline (cached per process).
///
/// The first call runs `calibrate(10)` and caches the result. All subsequent
/// calls return the cached baseline with zero overhead. This means all tests
/// in a single `cargo test` run share the same baseline.
pub fn get_calibration() -> &'static CalibrationBaseline {
    CALIBRATION.get_or_init(|| {
        let baseline = calibrate(CALIBRATION_ITERATIONS);
        println!(
            "\n[calibration] Baseline: {:.3}ms (CV: {:.1}%, {} iterations) | Default threshold: {:.3}ms ({}x, floor {}ms)\n",
            baseline.median_ms,
            baseline.cv_percent,
            baseline.iteration_count,
            baseline.default_threshold_ms(),
            DEFAULT_MULTIPLIER,
            DEFAULT_FLOOR_MS,
        );
        baseline
    })
}
