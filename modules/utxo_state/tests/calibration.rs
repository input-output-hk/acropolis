//! Calibration tests for UPLC performance test normalization.

mod common;
use acropolis_module_utxo_state::validations::phase2::evaluate_raw_flat_program;
use common::{
    calibrate, get_calibration, CalibrationBaseline, CALIBRATION_ITERATIONS, CALIBRATION_SCRIPT,
};

#[test]
fn calibration_script_evaluates() {
    let result = evaluate_raw_flat_program(CALIBRATION_SCRIPT);
    assert!(
        result.is_ok(),
        "Calibration script failed: {:?}",
        result.err()
    );
    let elapsed_ms = result.unwrap().as_secs_f64() * 1000.0;
    println!("Calibration script single run: {elapsed_ms:.3}ms");
    assert!(elapsed_ms > 0.0);
}

#[test]
fn calibration_stability() {
    let runs = 5;
    let mut medians: Vec<f64> = Vec::with_capacity(runs);

    println!("\n=== Calibration Stability Test ===");
    for i in 0..runs {
        let baseline = calibrate(CALIBRATION_ITERATIONS);
        println!(
            "  Run {}: median={:.3}ms, CV={:.1}%",
            i + 1,
            baseline.median_ms,
            baseline.cv_percent
        );
        assert!(
            baseline.cv_percent < 20.0,
            "Run {} CV too high: {:.1}%",
            i + 1,
            baseline.cv_percent
        );
        medians.push(baseline.median_ms);
    }

    medians.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let overall_median = medians[medians.len() / 2];
    let tolerance = overall_median * 0.30;

    for (i, m) in medians.iter().enumerate() {
        let diff = (m - overall_median).abs();
        assert!(
            diff <= tolerance,
            "Run {} median ({:.3}ms) deviates {:.3}ms from overall ({:.3}ms), exceeds ±30%",
            i + 1,
            m,
            diff,
            overall_median
        );
    }
    println!("  Result: PASS — all runs stable\n");
}

#[test]
fn threshold_computation() {
    let baseline = CalibrationBaseline {
        median_ms: 5.0,
        iteration_count: 10,
        cv_percent: 3.0,
    };

    assert_eq!(baseline.threshold_ms(5.0, 10.0), 25.0);
    assert_eq!(baseline.threshold_ms(1.0, 10.0), 10.0);
    assert_eq!(baseline.default_threshold_ms(), 50.0);
    assert_eq!(baseline.ratio(10.0), 2.0);
    assert_eq!(baseline.ratio(25.0), 5.0);
}

#[test]
fn get_calibration_cached() {
    let first = get_calibration();
    let second = get_calibration();
    assert!(
        std::ptr::eq(first, second),
        "get_calibration should return cached result"
    );
    assert!(first.median_ms > 0.0);
    assert!(first.cv_percent < 25.0);
}
