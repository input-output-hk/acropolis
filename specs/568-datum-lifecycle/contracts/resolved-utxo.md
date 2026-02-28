# Contract: ResolvedUTxO and Phase 2 Validation Interface

**Module**: `acropolis_module_plutus_validation`  
**Consumers**: `acropolis_module_utxo_state`, `acropolis_module_tx_unpacker`

## Public API

### Core Types

```rust
/// A script resolved from the transaction witness set or a reference UTxO.
pub struct ResolvedScript {
    pub hash: ScriptHash,
    pub language: ScriptLang,
    pub bytecode: Vec<u8>,
}

/// A datum resolved for Phase 2 evaluation.
pub struct ResolvedDatum {
    pub hash: DatumHash,
    pub bytes: Vec<u8>,
    pub source: DatumSource,
}

/// How a datum was obtained.
pub enum DatumSource {
    Inline,
    WitnessSet,
}

/// Complete resolved input for Phase 2 evaluation.
pub struct ResolvedInput {
    pub utxo_ref: UTxOIdentifier,
    pub address: Vec<u8>,
    pub value: UTxOValue,
    pub datum: Option<ResolvedDatum>,
    pub reference_script: Option<ResolvedScript>,
}

/// Result of Phase 2 validation for a single transaction.
pub struct Phase2Result {
    pub valid: bool,
    pub scripts_run: usize,
    pub total_cpu: u64,
    pub total_mem: u64,
    pub error: Option<Phase2Error>,
}
```

### Entry Point

```rust
/// Evaluate all scripts in a transaction.
///
/// # Arguments
/// * `resolved_inputs` - All consumed UTxOs with resolved datums
/// * `resolved_ref_inputs` - Reference inputs (V2+ only)
/// * `scripts` - Scripts to evaluate (from witness set or reference UTxOs)
/// * `redeemers` - Redeemers indexed by ScriptPurpose
/// * `tx_body_cbor` - Raw CBOR of the transaction body (for TxInfo)
/// * `cost_models` - Protocol parameter cost models per language version
/// * `ex_unit_prices` - ExUnit prices from protocol parameters
///
/// # Returns
/// Phase2Result indicating pass/fail and resource consumption.
///
/// # Errors
/// Returns Phase2Error if datum resolution, ScriptContext construction,
/// or script evaluation fails.
pub fn evaluate_transaction_phase2(
    resolved_inputs: &[ResolvedInput],
    resolved_ref_inputs: &[ResolvedInput],
    scripts: &[ResolvedScript],
    redeemers: &[(ScriptPurpose, Redeemer)],
    tx_body_cbor: &[u8],
    cost_models: &CostModels,
    ex_unit_prices: &ExUnitPrices,
) -> Phase2Result;
```

### ScriptContext Construction

```rust
/// Build a ScriptContext for a specific script and version.
///
/// Dispatches to V1, V2, or V3 construction based on language.
pub fn build_script_context(
    language: ScriptLang,
    resolved_inputs: &[ResolvedInput],
    resolved_ref_inputs: &[ResolvedInput],
    tx_body: &TransactionBody,
    purpose: &ScriptPurpose,
    redeemer: &Redeemer,
    datum: Option<&ResolvedDatum>,
    cost_models: &CostModels,
) -> Result<Vec<u8>, Phase2Error>;
```

### Datum Resolution

```rust
/// Resolve the datum for a consumed UTxO.
///
/// Algorithm:
/// 1. Check UTxO output for inline datum → use directly
/// 2. Check UTxO output for datum hash → find matching entry in witness set
/// 3. Validate hash: blake2b_256(datum_bytes) == declared_hash
/// 4. V1/V2 Spending: datum required → error if missing
/// 5. V3 Spending: datum optional (CIP-0069)
///
/// # Errors
/// - DatumNotFound if hash not in witness set (V1/V2)
/// - DatumHashMismatch if computed hash ≠ declared hash
pub fn resolve_datum(
    utxo: &UTXOValue,
    witness_datums: &[(DatumHash, Vec<u8>)],
    language: ScriptLang,
) -> Result<Option<ResolvedDatum>, Phase2Error>;
```

## Integration Contract: utxo_state

### TxUTxODeltas Extension

The `TxUTxODeltas` struct in `common/src/ledger_state.rs` gains:

```rust
pub struct TxUTxODeltas {
    // ... existing fields ...
    /// Raw transaction bytes for Phase 2 re-parsing.
    /// None for transactions without scripts.
    pub raw_tx: Option<Vec<u8>>,
}
```

### validate_block_utxos Integration Point

In `utxo_state/src/state.rs`, after Phase 1 validation:

```rust
// Existing Phase 1 validation
validate_consumed_utxos(&tx, &input_utxos)?;

// NEW: Phase 2 validation (if transaction has scripts)
if !tx.script_witnesses.is_empty() {
    if let Some(raw_tx) = &delta.raw_tx {
        let resolved_inputs = resolve_all_inputs(&tx, &input_utxos, &delta)?;
        let phase2_result = plutus_validation::evaluate_transaction_phase2(
            &resolved_inputs,
            &resolved_ref_inputs,
            &scripts,
            &redeemers,
            raw_tx,
            &cost_models,
            &ex_unit_prices,
        );
        if !phase2_result.valid {
            return handle_phase2_failure(&tx, &phase2_result);
        }
    }
}
```

## Invariants

1. **Datum hash integrity**: Every witness-set datum passes `blake2b_256(bytes) == hash`
2. **Version consistency**: ScriptContext version matches ScriptLang of the evaluated script
3. **Sequential Phase 1→2**: Phase 2 never runs before Phase 1 passes
4. **Parallel scripts**: Scripts within one transaction may evaluate concurrently
5. **Intra-block ordering**: Transaction $n$ can consume outputs from transaction $n-1$ in the same block
6. **Collateral handling**: Phase 2 failure → apply collateral inputs, discard normal inputs/outputs
