# Quickstart: Plutus Phase 2 Validation

**Feature**: 568-plutus-phase2-validation  
**Date**: 2026-02-06

## Overview

This guide covers how to integrate and use the Plutus Phase 2 validation feature in Acropolis.

---

## Prerequisites

1. **Rust 2024 Edition** toolchain
2. **uplc-turbo** crate (already in workspace Cargo.toml)
3. Protocol parameters with cost models available

---

## Quick Integration

### 1. Add Module Dependency

In `modules/tx_unpacker/Cargo.toml`:

```toml
[dependencies]
uplc-turbo = { workspace = true }
```

### 2. Enable Feature Flag

In your `omnibus.toml` configuration:

```toml
[module.tx-unpacker]
phase2_validation_enabled = true
```

Or leave disabled (default) for Phase 1 only validation.

---

## Basic Usage

### Evaluating a Single Script

```rust
use uplc_turbo::{
    arena::Arena,
    bumpalo::Bump,
    flat,
    binder::DeBruijn,
    machine::{PlutusVersion, ExBudget},
};

fn evaluate_plutus_script(
    script_bytes: &[u8],  // FLAT-encoded script (after CBOR unwrap)
    plutus_version: PlutusVersion,
    cost_model: &[i64],
    budget: ExBudget,
) -> Result<bool, String> {
    // Create arena with 1MB capacity
    let bump = Bump::with_capacity(1_024_000);
    let arena = Arena::from_bump(bump);
    
    // Decode script from FLAT format
    let program = flat::decode::<DeBruijn>(&arena, script_bytes)
        .map_err(|e| format!("Decode error: {e}"))?;
    
    // Evaluate with cost model and budget
    let result = program.eval_with_params(&arena, plutus_version, cost_model, budget);
    
    match result.term {
        Ok(_) => {
            println!("Script succeeded!");
            println!("CPU consumed: {}", result.info.consumed_budget.cpu);
            println!("Memory consumed: {}", result.info.consumed_budget.mem);
            Ok(true)
        }
        Err(e) => {
            println!("Script failed: {:?}", e);
            Ok(false)
        }
    }
}
```

### Evaluating a Spending Validator

Spending validators require 3 arguments: datum, redeemer, script context.

```rust
use uplc_turbo::{
    term::Term,
    constant::Constant,
    data::PlutusData,
};

fn evaluate_spending_validator(
    arena: &Arena,
    script_bytes: &[u8],
    datum: &PlutusData,
    redeemer: &PlutusData,
    script_context: &PlutusData,
    plutus_version: PlutusVersion,
    cost_model: &[i64],
    budget: ExBudget,
) -> Result<bool, String> {
    // Decode script
    let program = flat::decode::<DeBruijn>(arena, script_bytes)
        .map_err(|e| format!("Decode error: {e}"))?;
    
    // Create Term arguments from PlutusData
    let datum_term = Term::constant(arena, Constant::data(arena, datum));
    let redeemer_term = Term::constant(arena, Constant::data(arena, redeemer));
    let context_term = Term::constant(arena, Constant::data(arena, script_context));
    
    // Apply arguments to script
    let applied = program
        .apply(arena, datum_term)
        .apply(arena, redeemer_term)
        .apply(arena, context_term);
    
    // Evaluate
    let result = applied.eval_with_params(arena, plutus_version, cost_model, budget);
    
    Ok(result.term.is_ok())
}
```

### Evaluating a Minting Policy

Minting policies take 2 arguments: redeemer, script context.

```rust
fn evaluate_minting_policy(
    arena: &Arena,
    script_bytes: &[u8],
    redeemer: &PlutusData,
    script_context: &PlutusData,
    plutus_version: PlutusVersion,
    cost_model: &[i64],
    budget: ExBudget,
) -> Result<bool, String> {
    let program = flat::decode::<DeBruijn>(arena, script_bytes)
        .map_err(|e| format!("Decode error: {e}"))?;
    
    let redeemer_term = Term::constant(arena, Constant::data(arena, redeemer));
    let context_term = Term::constant(arena, Constant::data(arena, script_context));
    
    let applied = program
        .apply(arena, redeemer_term)
        .apply(arena, context_term);
    
    let result = applied.eval_with_params(arena, plutus_version, cost_model, budget);
    
    Ok(result.term.is_ok())
}
```

---

## Working with PlutusData

### Creating PlutusData

```rust
use uplc_turbo::data::PlutusData;

// Integer
let int_data = PlutusData::integer(arena, arena.alloc_integer(42.into()));

// ByteString
let bytes: &[u8] = &[0xde, 0xad, 0xbe, 0xef];
let bytes_data = PlutusData::byte_string(arena, bytes);

// Constructor (e.g., Just x)
let just_data = PlutusData::constr(arena, 0, &[int_data]);

// List
let list_data = PlutusData::list(arena, &[int_data, bytes_data]);

// Map
let map_data = PlutusData::map(arena, &[(int_data, bytes_data)]);
```

### Decoding PlutusData from CBOR

```rust
let cbor_bytes: &[u8] = /* datum/redeemer CBOR */;
let data = PlutusData::from_cbor(arena, cbor_bytes)?;
```

---

## Memory Management

The arena allocator is key to constant memory usage:

```rust
fn process_block_scripts(block_scripts: &[BlockScript]) {
    // Create arena once per block
    let mut arena = Arena::from_bump(Bump::with_capacity(2_000_000));
    
    for script in block_scripts {
        // Evaluate script...
        let result = evaluate_script(&arena, script);
        
        // Process result...
        
        // Reset arena between scripts (frees all allocations)
        arena.reset();
    }
}
```

---

## Parallel Evaluation

For blocks with multiple scripts, use rayon for parallel evaluation:

```rust
use rayon::prelude::*;

fn evaluate_scripts_parallel(scripts: &[ScriptRequest]) -> Vec<EvalResult> {
    scripts
        .par_iter()
        .map(|req| {
            // Each thread creates its own arena
            let bump = Bump::with_capacity(1_024_000);
            let arena = Arena::from_bump(bump);
            
            evaluate_single_script(&arena, req)
        })
        .collect()
}
```

---

## Cost Models

Get cost model from protocol parameters:

```rust
fn get_cost_model(params: &ProtocolParams, version: PlutusVersion) -> &[i64] {
    match version {
        PlutusVersion::V1 => &params.plutus_v1_cost_model,
        PlutusVersion::V2 => &params.plutus_v2_cost_model,
        PlutusVersion::V3 => &params.plutus_v3_cost_model,
    }
}
```

---

## Error Handling

```rust
use uplc_turbo::machine::MachineError;

fn handle_eval_result<'a>(result: EvalResult<'a>) -> ValidationOutcome {
    match result.term {
        Ok(_term) => {
            ValidationOutcome::Success {
                cpu_used: result.info.consumed_budget.cpu,
                mem_used: result.info.consumed_budget.mem,
            }
        }
        Err(MachineError::ExplicitErrorTerm) => {
            ValidationOutcome::ScriptFailed {
                reason: "Script called error builtin".to_string(),
            }
        }
        Err(MachineError::OutOfExError(budget)) => {
            ValidationOutcome::BudgetExceeded {
                cpu: budget.cpu,
                mem: budget.mem,
            }
        }
        Err(e) => {
            ValidationOutcome::MachineError {
                message: format!("{:?}", e),
            }
        }
    }
}
```

---

## Testing

### Unit Test Example

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_always_succeeds_script() {
        let arena = Arena::new();
        
        // Always succeeds script: (lam x (lam y (lam z ())))
        // FLAT encoded
        let script_bytes = hex::decode("4d01000033222220051").unwrap();
        
        let result = evaluate_script(
            &arena,
            &script_bytes,
            PlutusVersion::V2,
            &default_cost_model_v2(),
            ExBudget { cpu: 1_000_000, mem: 100_000 },
        );
        
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_always_fails_script() {
        let arena = Arena::new();
        
        // Always fails: error
        let script_bytes = hex::decode("4d01000033222220069").unwrap();
        
        let result = evaluate_script(
            &arena,
            &script_bytes,
            PlutusVersion::V2,
            &default_cost_model_v2(),
            ExBudget { cpu: 1_000_000, mem: 100_000 },
        );
        
        assert!(matches!(result, Err(ScriptError::ExplicitError)));
    }
}
```

---

## Troubleshooting

### Script Deserialization Failed

- Ensure you've unwrapped the CBOR layer before FLAT decoding
- Scripts in transactions are CBOR-wrapped: `decode_cbor(script) -> flat_bytes`

### Budget Exceeded

- Check protocol parameters for correct `max_tx_ex_units`
- Script may legitimately be too expensive for current limits

### Missing Datum

- For V1 scripts, datum must be in witness set
- For V2+, datum can be inline in UTxO

### Wrong Plutus Version

- Check script language tag from transaction
- V1 scripts use different cost model than V2/V3
