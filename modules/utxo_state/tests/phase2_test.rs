//! Phase 2 script execution integration tests.
//!
//! Tests the full execution pipeline: ScriptContext -> to_script_args -> decode -> apply -> eval.
//! Uses real mainnet Plutus scripts from fixtures for performance benchmarks.

mod common;

use acropolis_module_utxo_state::validations::phase2::evaluate_raw_flat_program;

// =============================================================================
// Real script fixtures: raw FLAT evaluation
// =============================================================================

const ESCROW_REDEEM: &[u8] =
    include_bytes!("../../../tests/fixtures/plutus_scripts/escrow-redeem_1-1.flat");
const UNISWAP: &[u8] =
    include_bytes!("../../../tests/fixtures/plutus_scripts/uniswap-3.flat");
const VESTING: &[u8] =
    include_bytes!("../../../tests/fixtures/plutus_scripts/vesting-1.flat");
const CROWDFUNDING: &[u8] =
    include_bytes!("../../../tests/fixtures/plutus_scripts/crowdfunding-success-1.flat");
const GAME_SM: &[u8] =
    include_bytes!("../../../tests/fixtures/plutus_scripts/game-sm-success_1-1.flat");
const TOKEN_ACCOUNT: &[u8] =
    include_bytes!("../../../tests/fixtures/plutus_scripts/token-account-1.flat");
const STABLECOIN: &[u8] =
    include_bytes!("../../../tests/fixtures/plutus_scripts/stablecoin_1-1.flat");
const MULTISIG_SM: &[u8] =
    include_bytes!("../../../tests/fixtures/plutus_scripts/multisig-sm-5.flat");

/// Verify all fixture scripts can be decoded and evaluated.
#[test]
fn all_fixture_scripts_evaluate_successfully() {
    let scripts: &[(&str, &[u8])] = &[
        ("escrow-redeem_1-1", ESCROW_REDEEM),
        ("uniswap-3", UNISWAP),
        ("vesting-1", VESTING),
        ("crowdfunding-success-1", CROWDFUNDING),
        ("game-sm-success_1-1", GAME_SM),
        ("token-account-1", TOKEN_ACCOUNT),
        ("stablecoin_1-1", STABLECOIN),
        ("multisig-sm-5", MULTISIG_SM),
    ];

    println!("\n=== Fixture Script Evaluation ===");
    for (name, bytes) in scripts {
        let result = evaluate_raw_flat_program(bytes);
        let elapsed_ms = result
            .as_ref()
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        println!("  {name:<30} {elapsed_ms:>8.3}ms  {}", if result.is_ok() { "OK" } else { "FAIL" });
        assert!(result.is_ok(), "{name} failed: {:?}", result.err());
    }
    println!("=================================\n");
}

/// Performance benchmark: all fixture scripts within calibrated threshold.
#[test]
fn fixture_scripts_within_performance_threshold() {
    let baseline = common::get_calibration();
    let threshold = baseline.default_threshold_ms();

    let scripts: &[(&str, &[u8])] = &[
        ("escrow-redeem_1-1", ESCROW_REDEEM),
        ("uniswap-3", UNISWAP),
        ("vesting-1", VESTING),
        ("crowdfunding-success-1", CROWDFUNDING),
        ("game-sm-success_1-1", GAME_SM),
        ("token-account-1", TOKEN_ACCOUNT),
        ("stablecoin_1-1", STABLECOIN),
        ("multisig-sm-5", MULTISIG_SM),
    ];

    println!("\n=== Performance Benchmark (threshold: {threshold:.1}ms) ===");
    for (name, bytes) in scripts {
        // Warmup
        let _ = evaluate_raw_flat_program(bytes);

        // Measure median of 5 runs
        let mut timings: Vec<f64> = (0..5)
            .map(|_| {
                evaluate_raw_flat_program(bytes)
                    .unwrap()
                    .as_secs_f64()
                    * 1000.0
            })
            .collect();
        timings.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = timings[timings.len() / 2];
        let ratio = baseline.ratio(median);

        println!(
            "  {name:<30} median={median:>8.3}ms  ratio={ratio:.1}x  {}",
            if median < threshold { "PASS" } else { "FAIL" }
        );
        assert!(
            median < threshold,
            "{name}: {median:.3}ms exceeds threshold {threshold:.1}ms"
        );
    }
    println!("=================================================\n");
}

/// Parallel evaluation: run all fixture scripts concurrently.
#[test]
fn parallel_fixture_evaluation() {
    use std::thread;

    let scripts: Vec<(&str, &[u8])> = vec![
        ("escrow-redeem_1-1", ESCROW_REDEEM),
        ("uniswap-3", UNISWAP),
        ("vesting-1", VESTING),
        ("crowdfunding-success-1", CROWDFUNDING),
        ("game-sm-success_1-1", GAME_SM),
        ("token-account-1", TOKEN_ACCOUNT),
        ("stablecoin_1-1", STABLECOIN),
        ("multisig-sm-5", MULTISIG_SM),
    ];

    let start = std::time::Instant::now();
    let handles: Vec<_> = scripts
        .into_iter()
        .map(|(name, bytes)| {
            let bytes = bytes.to_vec();
            let name = name.to_string();
            thread::spawn(move || {
                let result = evaluate_raw_flat_program(&bytes);
                assert!(result.is_ok(), "{name} failed: {:?}", result.err());
            })
        })
        .collect();

    for h in handles {
        h.join().expect("Thread panicked");
    }
    let elapsed = start.elapsed();

    println!(
        "\n  Parallel evaluation of 8 scripts: {:.3}ms\n",
        elapsed.as_secs_f64() * 1000.0
    );
}
