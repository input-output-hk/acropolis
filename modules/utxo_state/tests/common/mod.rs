//! Shared test infrastructure for UPLC performance test normalization.
//!
//! Provides a machine-adaptive calibration baseline for benchmark thresholds.
//! The calibration evaluates a known UPLC program through the same evaluator
//! pool used by script execution, ensuring the baseline correlates with actual
//! script evaluation performance.

use std::sync::OnceLock;

use acropolis_module_utxo_state::validations::phase2::evaluate_raw_flat_program;

/// Default number of calibration iterations.
pub const CALIBRATION_ITERATIONS: usize = 10;

/// Default performance multiplier (threshold = multiplier × baseline).
pub const DEFAULT_MULTIPLIER: f64 = 10.0;

/// Minimum absolute floor for thresholds (ms).
pub const DEFAULT_FLOOR_MS: f64 = 10.0;

/// The calibration script bytes (escrow-redeem_1-1.flat, ~5.6KB).
pub const CALIBRATION_SCRIPT: &[u8] =
    include_bytes!("../../../../tests/fixtures/plutus_scripts/escrow-redeem_1-1.flat");

/// Result of machine calibration.
#[derive(Debug, Clone)]
pub struct CalibrationBaseline {
    pub median_ms: f64,
    pub iteration_count: usize,
    pub cv_percent: f64,
}

impl CalibrationBaseline {
    pub fn threshold_ms(&self, multiplier: f64, floor_ms: f64) -> f64 {
        (self.median_ms * multiplier).max(floor_ms)
    }

    pub fn default_threshold_ms(&self) -> f64 {
        self.threshold_ms(DEFAULT_MULTIPLIER, DEFAULT_FLOOR_MS)
    }

    pub fn ratio(&self, elapsed_ms: f64) -> f64 {
        if self.median_ms > 0.0 {
            elapsed_ms / self.median_ms
        } else {
            f64::INFINITY
        }
    }
}

/// Run the calibration workload and return the baseline.
pub fn calibrate(iterations: usize) -> CalibrationBaseline {
    assert!(iterations >= 3, "Need at least 3 iterations for a meaningful median");

    // Warmup run (discarded)
    evaluate_raw_flat_program(CALIBRATION_SCRIPT).expect("Calibration warmup must succeed");

    let mut timings: Vec<f64> = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let elapsed = evaluate_raw_flat_program(CALIBRATION_SCRIPT)
            .expect("Calibration script must succeed");
        timings.push(elapsed.as_secs_f64() * 1000.0);
    }

    timings.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_ms = timings[timings.len() / 2];

    let mean = timings.iter().sum::<f64>() / timings.len() as f64;
    let variance = timings.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / timings.len() as f64;
    let stddev = variance.sqrt();
    let cv_percent = if mean > 0.0 { (stddev / mean) * 100.0 } else { 0.0 };

    CalibrationBaseline {
        median_ms,
        iteration_count: iterations,
        cv_percent,
    }
}

static CALIBRATION: OnceLock<CalibrationBaseline> = OnceLock::new();

/// Get or compute the calibration baseline (cached per process).
pub fn get_calibration() -> &'static CalibrationBaseline {
    CALIBRATION.get_or_init(|| {
        let baseline = calibrate(CALIBRATION_ITERATIONS);
        println!(
            "\n[calibration] Baseline: {:.3}ms (CV: {:.1}%, {} iters) | Threshold: {:.3}ms ({}x, floor {}ms)\n",
            baseline.median_ms, baseline.cv_percent, baseline.iteration_count,
            baseline.default_threshold_ms(), DEFAULT_MULTIPLIER, DEFAULT_FLOOR_MS,
        );
        baseline
    })
}
