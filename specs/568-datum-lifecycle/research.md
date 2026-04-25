# Research: Datum Lifecycle Management for Phase 2 Plutus Validation

**Feature**: `568-datum-lifecycle`  
**Date**: 2026-02-12  
**Status**: Complete

## Research Questions & Findings

### RQ-1: Where should Phase 2 validation run?

**Decision**: Phase 2 validation should run in `utxo_state`, with the evaluation logic in a shared library crate.

**Rationale**: `utxo_state` is the only module with direct access to the UTxO HashMap including datums and addresses. The existing `validate_block_utxos` loop already collects input UTxOs, iterates transactions sequentially, runs Phase 1, and accumulates outputs between transactions for intra-block resolution. Phase 2 slots in naturally after Phase 1 per-tx in this same loop. Per L007, Phase 2 must run sequentially after Phase 1 per transaction because Phase 2 results determine what gets applied (inputs vs collateral).

**Alternatives considered**:
- **tx_unpacker queries utxo_state via message bus**: Rejected — circular dependency (tx_unpacker runs before utxo_state processes the block), intra-block ordering problem (FR-009), and per-input async round-trip overhead.
- **Phase 2 only in tx_unpacker**: Rejected — impossible to resolve datums or build ScriptContext without UTxO state access.

### RQ-2: How to get script bytes into utxo_state?

**Decision**: Extend `TxUTxODeltas` to carry raw transaction bytes alongside the existing delta fields.

**Rationale**: Phase 2 needs Plutus script bytecodes from the transaction witness set. Currently `TxUTxODeltas` only carries `script_witnesses: Vec<(ScriptHash, ScriptLang)>` without the actual script bytes. Carrying raw tx bytes allows re-extraction of script bytes, witness-set datums, and redeemers in `utxo_state` without duplicating every field. The overhead is ~1-2KB per tx per block.

**Alternatives considered**:
- **Add separate `plutus_scripts: Vec<(ScriptHash, Vec<u8>)>` field**: More targeted but requires duplicating extraction logic and doesn't help with ScriptContext construction which needs many transaction fields.
- **Share raw bytes via Arc**: Could reduce copying overhead but adds lifetime complexity across the message bus.

### RQ-3: ScriptContext CBOR format per Plutus version

**Decision**: Build ScriptContext construction following the Amaru reference implementation patterns, with version-specific CBOR encoding.

**Rationale**: The three Plutus versions have significantly different ScriptContext structures:

| Aspect | V1 | V2 | V3 |
|--------|----|----|-----|
| Script arguments | datum, redeemer, context (2-3 args) | datum, redeemer, context (2-3 args) | context only (1 arg) |
| ScriptContext wrapper | `[TxInfo, ScriptPurpose]` | `[TxInfo, ScriptPurpose]` | `[TxInfo, Redeemer, ScriptInfo]` |
| TxInfo fields | 10 | 12 (+ref inputs, redeemers) | 16 (+governance fields) |
| TxOut fields | 3 (addr, value, datum_hash) | 4 (+inline datum, ref script) | 4 |
| Fee encoding | Value (Map) | Value (Map) | Plain Integer |
| TxId encoding | Wrapped in `Constr 0` | Wrapped in `Constr 0` | Plain ByteString |
| Datum in TxOut | `Maybe DatumHash` | `OutputDatum` (3 variants) | `OutputDatum` (3 variants) |
| Governance fields | None | None | votes, proposals, treasury |

The `amaru-plutus` crate in the Amaru project is the definitive Rust reference implementation. It could serve as a dependency or implementation guide.

**Alternatives considered**:
- **Depend on amaru-plutus directly**: Possible but introduces a dependency on the Amaru project which may have different release cadences.
- **Minimal ScriptContext (spending only)**: Could start with just V1/V2 spending context, but V3's single-argument model requires full context from the start.

### RQ-4: V3 argument application model

**Decision**: The evaluator must handle V3 differently — V3 scripts receive a single argument (the ScriptContext, which embeds redeemer and datum inside ScriptInfo).

**Rationale**: Current evaluator applies `program.apply(datum).apply(redeemer).apply(context)` for spending and `program.apply(redeemer).apply(context)` for other purposes. V3 expects `program.apply(context)` only — the datum is embedded in the `Spending` variant of `ScriptInfo` within the context, and the redeemer is a top-level field of `ScriptContext`. This is a breaking change in argument application semantics.

### RQ-5: Datum resolution algorithm

**Decision**: Two-path resolution based on the datum variant stored on the UTxO entry.

**Rationale**: The algorithm is well-defined by the Cardano ledger spec:
1. Look up the consumed UTxO in the store → get `UTXOValue.datum`
2. If `Datum::Inline(bytes)` → use `bytes` directly
3. If `Datum::Hash(h)` → look up `h` in the transaction's `plutus_data` witness set → use the matching bytes
4. If `None` and script is V1/V2 → error (spending validators require datum)
5. If `None` and script is V3 → allowed (CIP-0069)
6. Validate: if `Datum::Hash(h)`, verify `blake2b_256(found_bytes) == h`

### RQ-6: What existing code can be reused?

**Decision**: Reuse extensively from both existing Acropolis code and the Phase 2 evaluator.

| Component | Status | Reuse |
|-----------|--------|-------|
| `Datum` enum | ✅ Exists in `common/src/datum.rs` | Direct reuse (per L006) |
| `DatumHash` type | ✅ Exists in `common/src/hash.rs` | Direct reuse |
| `UTXOValue.datum` | ✅ Already stores datums | Direct lookup |
| `Transaction.plutus_data` | ✅ Witness set datums | Direct lookup for hash resolution |
| `resolve_scripts_needed` | ✅ In `common/src/ledger_state.rs` | Determines which scripts need Phase 2 |
| Phase 2 evaluator | ✅ In `tx_unpacker/validations/phase2.rs` | Move to shared crate |
| Arena pool + thread pool | ✅ In `tx_unpacker/validations/phase2.rs` | Move with evaluator |
| `validate_block_utxos` loop | ✅ In `utxo_state/src/state.rs` | Add Phase 2 call site |
| Alonzo UTxOW stubs | ⚠️ Stubs in `utxo_state/validations/alonzo/` | Implement datum validation |

### RQ-7: Cost models availability

**Decision**: Cost models are already available in `utxo_state` via protocol parameters subscription.

**Rationale**: `utxo_state` subscribes to `ProtocolParameters` messages and stores them. The cost model parameters needed by the evaluator (Plutus V1/V2/V3 cost model vectors) are part of protocol parameters. Currently `tx_unpacker`'s Phase 2 returns all-zero cost models — `utxo_state` can provide the real ones.
