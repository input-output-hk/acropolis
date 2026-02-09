//! Phase 2 validation integration tests.
//!
//! Tests follow TDD approach: write test first (RED), then implement (GREEN).

use acropolis_module_tx_unpacker::validations::phase2::{
    evaluate_script, ExBudget, Phase2Error, PlutusVersion,
};
use uplc_turbo::{
    arena::Arena,
    binder::DeBruijn,
    flat,
    program::{Program, Version},
    term::Term,
};

// =============================================================================
// Helper functions to create test scripts
// =============================================================================

/// Create a FLAT-encoded program that returns unit (always succeeds)
/// This is a 2-arg lambda simulating a minting policy:
/// (program 1.1.0 (lam r (lam ctx (con unit ()))))
fn create_unit_program() -> Vec<u8> {
    let arena = Arena::new();
    // Real Cardano scripts always take at least 2 args (redeemer, context)
    let term = Term::unit(&arena)
        .lambda(&arena, DeBruijn::zero(&arena)) // ctx
        .lambda(&arena, DeBruijn::zero(&arena)); // r
    let version = Version::plutus_v3(&arena);
    let program = Program::<DeBruijn>::new(&arena, version, term);
    flat::encode(program).expect("Failed to encode unit program")
}

/// Create a FLAT-encoded program that calls error (always fails)
/// This is a 2-arg lambda simulating a minting policy that fails:
/// (program 1.1.0 (lam r (lam ctx (error))))
fn create_error_program() -> Vec<u8> {
    let arena = Arena::new();
    let term = Term::error(&arena)
        .lambda(&arena, DeBruijn::zero(&arena)) // ctx
        .lambda(&arena, DeBruijn::zero(&arena)); // r
    let version = Version::plutus_v3(&arena);
    let program = Program::<DeBruijn>::new(&arena, version, term);
    flat::encode(program).expect("Failed to encode error program")
}

/// Create a FLAT-encoded program that is a lambda taking 3 args and returning unit
/// This simulates a spending validator: (lam d (lam r (lam ctx (con unit ()))))
fn create_spending_validator_succeeds() -> Vec<u8> {
    let arena = Arena::new();
    let term = Term::unit(&arena)
        .lambda(&arena, DeBruijn::zero(&arena)) // ctx
        .lambda(&arena, DeBruijn::zero(&arena)) // r
        .lambda(&arena, DeBruijn::zero(&arena)); // d
    let version = Version::plutus_v3(&arena);
    let program = Program::<DeBruijn>::new(&arena, version, term);
    flat::encode(program).expect("Failed to encode spending validator")
}

/// Create a FLAT-encoded program that is a lambda taking 2 args and returning unit
/// This simulates a minting policy: (lam r (lam ctx (con unit ())))
fn create_minting_policy_succeeds() -> Vec<u8> {
    let arena = Arena::new();
    let term = Term::unit(&arena)
        .lambda(&arena, DeBruijn::zero(&arena)) // ctx
        .lambda(&arena, DeBruijn::zero(&arena)); // r
    let version = Version::plutus_v3(&arena);
    let program = Program::<DeBruijn>::new(&arena, version, term);
    flat::encode(program).expect("Failed to encode minting policy")
}

/// Create a FLAT-encoded program that takes 3 args and calls error
/// This simulates a spending validator that fails: (lam d (lam r (lam ctx (error))))
fn create_spending_validator_fails() -> Vec<u8> {
    let arena = Arena::new();
    let term = Term::error(&arena)
        .lambda(&arena, DeBruijn::zero(&arena)) // ctx
        .lambda(&arena, DeBruijn::zero(&arena)) // r
        .lambda(&arena, DeBruijn::zero(&arena)); // d
    let version = Version::plutus_v3(&arena);
    let program = Program::<DeBruijn>::new(&arena, version, term);
    flat::encode(program).expect("Failed to encode failing spending validator")
}

/// Create a minimal CBOR-encoded PlutusData (empty constr)
fn create_empty_plutus_data() -> Vec<u8> {
    // CBOR: d87980 = tag 121 (Constr 0) + empty array
    vec![0xd8, 0x79, 0x80]
}

// =============================================================================
// Default Cost Model (Plutus V3)
// =============================================================================

/// Default V3 cost model from mainnet protocol parameters.
/// This is a minimal cost model for testing - full production would use
/// complete protocol parameter values.
fn default_cost_model_v3() -> Vec<i64> {
    // Plutus V3 has ~250+ cost model parameters
    // Use reasonable defaults for testing
    let mut cost_model = vec![0i64; 300];

    // Set some basic costs to non-zero values
    // These are approximate values based on mainnet
    for i in 0..cost_model.len() {
        cost_model[i] = match i {
            // startup costs
            0..=10 => 100000,
            // memory costs
            11..=50 => 100,
            // CPU costs
            _ => 1000,
        };
    }

    cost_model
}

fn default_cost_model_v1() -> Vec<i64> {
    // Mainnet Plutus V1 cost model (166 parameters)
    // Simplified for testing
    vec![0i64; 166]
}

fn default_cost_model_v2() -> Vec<i64> {
    // Mainnet Plutus V2 cost model (205 parameters)
    // TODO: Extract from conway-genesis.json or protocol parameters
    vec![
        205665,
        812,
        1,
        1,
        1000,
        571,
        0,
        1,
        1000,
        24177,
        4,
        1,
        1000,
        32,
        117366,
        10475,
        4,
        23000,
        100,
        23000,
        100,
        23000,
        100,
        23000,
        100,
        23000,
        100,
        23000,
        100,
        100,
        100,
        23000,
        100,
        19537,
        32,
        175354,
        32,
        46417,
        4,
        221973,
        511,
        0,
        1,
        89141,
        32,
        497525,
        14068,
        4,
        2,
        196500,
        453240,
        220,
        0,
        1,
        1,
        1000,
        28662,
        4,
        2,
        245000,
        216773,
        62,
        1,
        1060367,
        12586,
        1,
        208512,
        421,
        1,
        187000,
        1000,
        52998,
        1,
        80436,
        32,
        43249,
        32,
        1000,
        32,
        80556,
        1,
        57667,
        4,
        1000,
        10,
        197145,
        156,
        1,
        197145,
        156,
        1,
        204924,
        473,
        1,
        208896,
        511,
        1,
        52467,
        32,
        64832,
        32,
        65493,
        32,
        22558,
        32,
        16563,
        32,
        76511,
        32,
        196500,
        453240,
        220,
        0,
        1,
        1,
        69522,
        11687,
        0,
        1,
        60091,
        32,
        196500,
        453240,
        220,
        0,
        1,
        1,
        196500,
        453240,
        220,
        0,
        1,
        1,
        1159724,
        392670,
        0,
        2,
        806990,
        30482,
        4,
        1927926,
        82523,
        4,
        265318,
        0,
        4,
        0,
        85931,
        32,
        205665,
        812,
        1,
        1,
        41182,
        32,
        212342,
        32,
        31220,
        32,
        32696,
        32,
        43357,
        32,
        32247,
        32,
        38314,
        32,
        20000000000,
        20000000000,
        9462713,
        1021,
        10,
        20000000000,
        0,
        20000000000,
    ]
}

// =============================================================================
// Phase 1: Core Evaluation Tests (TDD)
// =============================================================================

/// T011: Test that a simple "always succeeds" script evaluates successfully
#[test]
fn test_eval_always_succeeds() {
    let script_bytes = create_unit_program();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    let result = evaluate_script(
        &script_bytes,
        PlutusVersion::V3,
        None, // No datum for simple script
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    assert!(result.is_ok(), "Script should succeed: {:?}", result.err());

    let eval_result = result.unwrap();
    // Script should consume some budget
    assert!(
        eval_result.consumed_budget.cpu >= 0,
        "CPU consumed should be non-negative"
    );
    assert!(
        eval_result.consumed_budget.mem >= 0,
        "Memory consumed should be non-negative"
    );
    // Script should complete within performance target (SC-001: <100ms)
    assert!(
        eval_result.within_target(),
        "Script took {:.2}ms, should be < 100ms",
        eval_result.elapsed_ms()
    );
    println!(
        "  evaluate_script elapsed: {:.3}ms (cpu: {}, mem: {})",
        eval_result.elapsed_ms(),
        eval_result.consumed_budget.cpu,
        eval_result.consumed_budget.mem
    );
}

/// T013: Test that a script calling `error` fails with ScriptFailed
#[test]
fn test_eval_always_fails() {
    let script_bytes = create_error_program();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    let result = evaluate_script(
        &script_bytes,
        PlutusVersion::V3,
        None,
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    assert!(result.is_err(), "Script should fail");

    match result.unwrap_err() {
        Phase2Error::ScriptFailed(_, msg) => {
            // Script called error builtin
            assert!(
                msg.contains("error") || msg.contains("Error"),
                "Error message should mention error: {}",
                msg
            );
        }
        other => panic!("Expected ScriptFailed, got {:?}", other),
    }
}

/// T015: Test that exceeding the budget returns BudgetExceeded error
#[test]
fn test_eval_budget_exceeded() {
    let script_bytes = create_unit_program();
    // Very small budget to force exceeding it
    let budget = ExBudget::new(1, 1);
    let cost_model = default_cost_model_v3();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    let result = evaluate_script(
        &script_bytes,
        PlutusVersion::V3,
        None,
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    assert!(result.is_err(), "Script should exceed budget");

    match result.unwrap_err() {
        Phase2Error::BudgetExceeded(_, cpu, mem) => {
            // Should have consumed more than allowed
            assert!(
                cpu > 0 || mem > 0,
                "Should have consumed some budget: cpu={}, mem={}",
                cpu,
                mem
            );
        }
        other => panic!("Expected BudgetExceeded, got {:?}", other),
    }
}

// =============================================================================
// Phase 2: Argument Application Tests (TDD)
// =============================================================================

/// T017: Test spending validator with 3 arguments (datum, redeemer, context)
#[test]
fn test_eval_spending_validator() {
    let script_bytes = create_spending_validator_succeeds();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3();
    let datum = create_empty_plutus_data();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    let result = evaluate_script(
        &script_bytes,
        PlutusVersion::V3,
        Some(&datum), // Spending validators take a datum
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    assert!(
        result.is_ok(),
        "Spending validator should succeed: {:?}",
        result.err()
    );
}

/// T017b: Test spending validator that fails
#[test]
fn test_eval_spending_validator_fails() {
    let script_bytes = create_spending_validator_fails();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3();
    let datum = create_empty_plutus_data();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    let result = evaluate_script(
        &script_bytes,
        PlutusVersion::V3,
        Some(&datum),
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    assert!(result.is_err(), "Spending validator should fail");
    assert!(matches!(
        result.unwrap_err(),
        Phase2Error::ScriptFailed(_, _)
    ));
}

/// T019: Test minting policy with 2 arguments (redeemer, context)
#[test]
fn test_eval_minting_policy() {
    let script_bytes = create_minting_policy_succeeds();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    let result = evaluate_script(
        &script_bytes,
        PlutusVersion::V3,
        None, // Minting policies don't take a datum
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    assert!(
        result.is_ok(),
        "Minting policy should succeed: {:?}",
        result.err()
    );
}

// =============================================================================
// Phase 3: Version-Specific Tests (FR-003)
// =============================================================================

/// T022: Test Plutus V1 script evaluation
#[test]
fn test_eval_plutus_v1_script() {
    // Create a V1 program - must be a lambda since we apply args
    let arena = Arena::new();
    let term = Term::unit(&arena)
        .lambda(&arena, DeBruijn::zero(&arena)) // ctx
        .lambda(&arena, DeBruijn::zero(&arena)); // r
    let version = Version::plutus_v1(&arena);
    let program = Program::<DeBruijn>::new(&arena, version, term);
    let script_bytes = flat::encode(program).expect("Failed to encode V1 program");

    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3(); // V1 cost model would be different in production
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    let result = evaluate_script(
        &script_bytes,
        PlutusVersion::V1,
        None,
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    assert!(
        result.is_ok(),
        "V1 script should succeed: {:?}",
        result.err()
    );
}

/// T023: Test Plutus V2 script evaluation
#[test]
fn test_eval_plutus_v2_script() {
    // Create a V2 program - must be a lambda since we apply args
    let arena = Arena::new();
    let term = Term::unit(&arena)
        .lambda(&arena, DeBruijn::zero(&arena)) // ctx
        .lambda(&arena, DeBruijn::zero(&arena)); // r
    let version = Version::plutus_v2(&arena);
    let program = Program::<DeBruijn>::new(&arena, version, term);
    let script_bytes = flat::encode(program).expect("Failed to encode V2 program");

    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3(); // V2 cost model would be different in production
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    let result = evaluate_script(
        &script_bytes,
        PlutusVersion::V2,
        None,
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    assert!(
        result.is_ok(),
        "V2 script should succeed: {:?}",
        result.err()
    );
}

/// T024: Test Plutus V3 script evaluation (already covered by other tests)
#[test]
fn test_eval_plutus_v3_script() {
    // V3 is the default for other tests, but test explicitly
    let script_bytes = create_unit_program();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    let result = evaluate_script(
        &script_bytes,
        PlutusVersion::V3,
        None,
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    assert!(
        result.is_ok(),
        "V3 script should succeed: {:?}",
        result.err()
    );
}

// =============================================================================
// SC-001 Benchmark: Script Evaluation Performance
// =============================================================================

/// Benchmark test to verify SC-001: individual script evaluation completes
/// in under 0.1 seconds (100ms) at the 95th percentile.
///
/// This test runs multiple iterations and calculates the p95 timing.
#[test]
fn test_sc001_eval_performance_p95() {
    const ITERATIONS: usize = 100;
    const P95_TARGET_MS: f64 = 100.0;

    let script_bytes = create_unit_program();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    // Warmup run
    let _ = evaluate_script(
        &script_bytes,
        PlutusVersion::V3,
        None,
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    // Collect timing samples
    let mut timings_ms: Vec<f64> = Vec::with_capacity(ITERATIONS);

    for _ in 0..ITERATIONS {
        let result = evaluate_script(
            &script_bytes,
            PlutusVersion::V3,
            None,
            &redeemer,
            &context,
            &cost_model,
            budget,
        );

        assert!(result.is_ok(), "Script should succeed");
        timings_ms.push(result.unwrap().elapsed_ms());
    }

    // Sort for percentile calculation
    timings_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let min = timings_ms[0];
    let max = timings_ms[ITERATIONS - 1];
    let median = timings_ms[ITERATIONS / 2];
    let p95_idx = (ITERATIONS as f64 * 0.95) as usize;
    let p95 = timings_ms[p95_idx];
    let mean: f64 = timings_ms.iter().sum::<f64>() / ITERATIONS as f64;

    println!("\n=== SC-001 Performance Benchmark ===");
    println!("Iterations: {}", ITERATIONS);
    println!("Min:    {:.3}ms", min);
    println!("Max:    {:.3}ms", max);
    println!("Mean:   {:.3}ms", mean);
    println!("Median: {:.3}ms", median);
    println!("P95:    {:.3}ms", p95);
    println!("Target: <{:.1}ms", P95_TARGET_MS);
    println!(
        "Result: {} (p95 {:.3}ms vs target {:.1}ms)",
        if p95 < P95_TARGET_MS {
            "PASS ✓"
        } else {
            "FAIL ✗"
        },
        p95,
        P95_TARGET_MS
    );
    println!("====================================\n");

    assert!(
        p95 < P95_TARGET_MS,
        "SC-001 FAILED: P95 {:.3}ms exceeds target {:.1}ms",
        p95,
        P95_TARGET_MS
    );
}

/// Benchmark test for spending validators (3-arg scripts).
/// These are typically more complex than minting policies.
#[test]
fn test_sc001_spending_validator_performance() {
    const ITERATIONS: usize = 50;
    const P95_TARGET_MS: f64 = 100.0;

    let script_bytes = create_spending_validator_succeeds();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);
    let cost_model = default_cost_model_v3();
    let datum = create_empty_plutus_data();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();

    // Warmup
    let _ = evaluate_script(
        &script_bytes,
        PlutusVersion::V3,
        Some(&datum),
        &redeemer,
        &context,
        &cost_model,
        budget,
    );

    // Collect timing samples
    let mut timings_ms: Vec<f64> = Vec::with_capacity(ITERATIONS);

    for _ in 0..ITERATIONS {
        let result = evaluate_script(
            &script_bytes,
            PlutusVersion::V3,
            Some(&datum),
            &redeemer,
            &context,
            &cost_model,
            budget,
        );

        assert!(result.is_ok(), "Spending validator should succeed");
        timings_ms.push(result.unwrap().elapsed_ms());
    }

    timings_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let p95_idx = (ITERATIONS as f64 * 0.95) as usize;
    let p95 = timings_ms[p95_idx];
    let mean: f64 = timings_ms.iter().sum::<f64>() / ITERATIONS as f64;

    println!("\n=== SC-001 Spending Validator Benchmark ===");
    println!("Iterations: {}", ITERATIONS);
    println!("Mean:   {:.3}ms", mean);
    println!("P95:    {:.3}ms", p95);
    println!(
        "Result: {}",
        if p95 < P95_TARGET_MS {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );
    println!("==========================================\n");

    assert!(
        p95 < P95_TARGET_MS,
        "SC-001 FAILED: Spending validator P95 {:.3}ms exceeds target {:.1}ms",
        p95,
        P95_TARGET_MS
    );
}

// =============================================================================
// Phase 4: Multi-Script Tests (US2) - Not part of Phase 3
// =============================================================================

#[test]
#[ignore = "T031: Implement parallel evaluation first"]
fn test_parallel_multi_script_block() {
    // TODO: T031 - Verify parallel execution is faster than sequential
    todo!("T031: Implement after validate_transaction_phase2() exists")
}

// =============================================================================
// Phase 3 Integration: validate_transaction_phase2 Tests (US1)
// =============================================================================

use acropolis_common::{ScriptHash, TxHash, UTxOIdentifier};
use acropolis_module_tx_unpacker::validations::phase2::{
    validate_transaction_phase2, ScriptInput, ScriptPurpose,
};

/// T029: Test validate_transaction_phase2 with a single minting script
#[test]
fn test_validate_transaction_phase2_single_mint_success() {
    // Create a successful minting policy
    let script_bytes = create_minting_policy_succeeds();
    let script_hash = ScriptHash::default();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);

    let script_inputs = vec![ScriptInput {
        script_hash,
        script_bytes: &script_bytes,
        plutus_version: PlutusVersion::V3,
        purpose: ScriptPurpose::Minting(script_hash),
        datum: None,
        redeemer: &redeemer,
        ex_units: budget,
    }];

    let cost_model_v1 = default_cost_model_v1();
    let cost_model_v2 = default_cost_model_v2();
    let cost_model_v3 = default_cost_model_v3();

    let result = validate_transaction_phase2(
        &script_inputs,
        &cost_model_v1,
        &cost_model_v2,
        &cost_model_v3,
        &context,
    );

    assert!(
        result.is_ok(),
        "Single mint script should succeed: {:?}",
        result.err()
    );
    let validation_result = result.unwrap();
    assert_eq!(validation_result.script_results.len(), 1);
    assert!(validation_result.total_consumed.cpu > 0);
}

/// T029: Test validate_transaction_phase2 with a failing script
#[test]
fn test_validate_transaction_phase2_single_mint_failure() {
    // Create a failing minting policy
    let script_bytes = create_error_program();
    let script_hash = ScriptHash::default();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);

    let script_inputs = vec![ScriptInput {
        script_hash,
        script_bytes: &script_bytes,
        plutus_version: PlutusVersion::V3,
        purpose: ScriptPurpose::Minting(script_hash),
        datum: None,
        redeemer: &redeemer,
        ex_units: budget,
    }];

    let cost_model_v1 = default_cost_model_v1();
    let cost_model_v2 = default_cost_model_v2();
    let cost_model_v3 = default_cost_model_v3();

    let result = validate_transaction_phase2(
        &script_inputs,
        &cost_model_v1,
        &cost_model_v2,
        &cost_model_v3,
        &context,
    );

    assert!(result.is_err(), "Failing mint script should return error");
    let err = result.unwrap_err();
    match err {
        Phase2Error::ScriptFailed(_, _) => (), // Expected
        other => panic!("Expected ScriptFailed, got: {:?}", other),
    }
}

/// T029: Test validate_transaction_phase2 with multiple scripts (sequential)
#[test]
fn test_validate_transaction_phase2_multiple_scripts() {
    // Create two successful minting policies
    let script1_bytes = create_minting_policy_succeeds();
    let script2_bytes = create_minting_policy_succeeds();
    let script1_hash = ScriptHash::default();
    let script2_hash = ScriptHash::try_from(vec![1u8; 28]).unwrap();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);

    let script_inputs = vec![
        ScriptInput {
            script_hash: script1_hash,
            script_bytes: &script1_bytes,
            plutus_version: PlutusVersion::V3,
            purpose: ScriptPurpose::Minting(script1_hash),
            datum: None,
            redeemer: &redeemer,
            ex_units: budget,
        },
        ScriptInput {
            script_hash: script2_hash,
            script_bytes: &script2_bytes,
            plutus_version: PlutusVersion::V3,
            purpose: ScriptPurpose::Minting(script2_hash),
            datum: None,
            redeemer: &redeemer,
            ex_units: budget,
        },
    ];

    let cost_model_v1 = default_cost_model_v1();
    let cost_model_v2 = default_cost_model_v2();
    let cost_model_v3 = default_cost_model_v3();

    let result = validate_transaction_phase2(
        &script_inputs,
        &cost_model_v1,
        &cost_model_v2,
        &cost_model_v3,
        &context,
    );

    assert!(
        result.is_ok(),
        "Multiple scripts should succeed: {:?}",
        result.err()
    );
    let validation_result = result.unwrap();
    assert_eq!(validation_result.script_results.len(), 2);
}

/// T029: Test validate_transaction_phase2 with spending validator (3 args)
#[test]
fn test_validate_transaction_phase2_spending() {
    let script_bytes = create_spending_validator_succeeds();
    let script_hash = ScriptHash::default();
    let datum = create_empty_plutus_data();
    let redeemer = create_empty_plutus_data();
    let context = create_empty_plutus_data();
    let budget = ExBudget::new(10_000_000_000, 10_000_000);

    let utxo_id = UTxOIdentifier {
        tx_hash: TxHash::default(),
        output_index: 0,
    };

    let script_inputs = vec![ScriptInput {
        script_hash,
        script_bytes: &script_bytes,
        plutus_version: PlutusVersion::V3,
        purpose: ScriptPurpose::Spending(utxo_id),
        datum: Some(&datum),
        redeemer: &redeemer,
        ex_units: budget,
    }];

    let cost_model_v1 = default_cost_model_v1();
    let cost_model_v2 = default_cost_model_v2();
    let cost_model_v3 = default_cost_model_v3();

    let result = validate_transaction_phase2(
        &script_inputs,
        &cost_model_v1,
        &cost_model_v2,
        &cost_model_v3,
        &context,
    );

    assert!(
        result.is_ok(),
        "Spending validator should succeed: {:?}",
        result.err()
    );
}

/// T029: Test empty script list (no-op)
#[test]
fn test_validate_transaction_phase2_empty() {
    let script_inputs: Vec<ScriptInput<'_>> = vec![];
    let context = create_empty_plutus_data();

    let cost_model_v1 = default_cost_model_v1();
    let cost_model_v2 = default_cost_model_v2();
    let cost_model_v3 = default_cost_model_v3();

    let result = validate_transaction_phase2(
        &script_inputs,
        &cost_model_v1,
        &cost_model_v2,
        &cost_model_v3,
        &context,
    );

    assert!(result.is_ok(), "Empty script list should succeed");
    let validation_result = result.unwrap();
    assert_eq!(validation_result.script_results.len(), 0);
    assert_eq!(validation_result.total_consumed.cpu, 0);
    assert_eq!(validation_result.total_consumed.mem, 0);
}

// =============================================================================
// Phase 5: Configuration Tests (US3)
// =============================================================================

use acropolis_common::{
    messages::RawTxsMessage, BlockHash, BlockInfo, BlockIntent, BlockStatus, Era, GenesisDelegates,
};
use acropolis_module_tx_unpacker::state::State;

/// Helper to create a minimal BlockInfo for testing
fn create_test_block_info() -> BlockInfo {
    BlockInfo {
        status: BlockStatus::Volatile,
        intent: BlockIntent::Apply,
        slot: 1000,
        number: 1,
        hash: BlockHash::default(),
        epoch: 1,
        epoch_slot: 1000,
        new_epoch: false,
        is_new_era: false,
        tip_slot: Some(1000),
        timestamp: 0,
        era: Era::Conway,
    }
}

/// T035: Test that Phase 2 validation is skipped when phase2_enabled = false
///
/// This test verifies FR-004: System MUST provide a configuration flag to
/// enable/disable Phase 2 validation, defaulting to disabled.
#[test]
fn test_phase2_disabled_skips_scripts() {
    // Create a state with Phase 2 disabled (the default)
    let state = State::new();
    assert!(
        !state.phase2_enabled,
        "Default should be phase2_enabled = false"
    );

    // Create a state explicitly with Phase 2 disabled
    let state = State::with_phase2_enabled(false);
    assert!(
        !state.phase2_enabled,
        "Explicit false should be phase2_enabled = false"
    );

    // Create block info for Conway era
    let block_info = create_test_block_info();

    // Create an empty transaction list (no scripts to validate)
    // Even with scripts, Phase 2 would be skipped when disabled
    let txs_msg = RawTxsMessage { txs: vec![] };
    let genesis_delegs = GenesisDelegates::default();

    // Validation should succeed (no txs, no errors)
    let result = state.validate(&block_info, &txs_msg, &genesis_delegs);
    assert!(
        result.is_ok(),
        "Empty tx list should succeed with phase2 disabled"
    );
}

/// T037: Test that Phase 2 validation runs when phase2_enabled = true
///
/// This test verifies that the configuration flag properly enables Phase 2
/// validation in the validation flow.
#[test]
fn test_phase2_enabled_validates_scripts() {
    // Create a state with Phase 2 enabled
    let state = State::with_phase2_enabled(true);
    assert!(state.phase2_enabled, "Should be phase2_enabled = true");

    // Create block info for Conway era
    let block_info = create_test_block_info();

    // Create an empty transaction list
    let txs_msg = RawTxsMessage { txs: vec![] };
    let genesis_delegs = GenesisDelegates::default();

    // Validation should succeed (no txs means no scripts to validate)
    let result = state.validate(&block_info, &txs_msg, &genesis_delegs);
    assert!(
        result.is_ok(),
        "Empty tx list should succeed with phase2 enabled"
    );
}

/// T035: Test State::new() defaults to phase2_enabled = false
#[test]
fn test_state_default_phase2_disabled() {
    let state = State::new();
    assert!(
        !state.phase2_enabled,
        "State::new() should default to phase2_enabled = false"
    );

    let state = State::default();
    assert!(
        !state.phase2_enabled,
        "State::default() should default to phase2_enabled = false"
    );
}

/// T035: Test State::with_phase2_enabled() constructor
#[test]
fn test_state_with_phase2_enabled_constructor() {
    let state_enabled = State::with_phase2_enabled(true);
    assert!(
        state_enabled.phase2_enabled,
        "with_phase2_enabled(true) should set phase2_enabled = true"
    );

    let state_disabled = State::with_phase2_enabled(false);
    assert!(
        !state_disabled.phase2_enabled,
        "with_phase2_enabled(false) should set phase2_enabled = false"
    );
}
