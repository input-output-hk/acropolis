# Phase 1 Data Model: Script Evaluation Visualizer

This feature introduces three data shapes and one channel. Two of the shapes live in `acropolis_common` (so they can be shared by `utxo_state` and the visualizer module without an extra crate dep); one is internal to the visualizer module.

## 1. `ScriptEvaluationOutcome`  *(new, `common/src/validation/phase2.rs`)*

A single Plutus script's evaluation result. Built inside `evaluate_scripts` once per `ScriptContext`.

| Field | Type | Source | Notes |
|---|---|---|---|
| `script_hash` | `ScriptHash` (28-byte) | `ScriptContext::script_hash` | Identifies which script executed. |
| `purpose` | `RedeemerTag` | `ScriptContext::redeemer.tag` | One of `Spend`, `Mint`, `Cert`, `Reward`, `Vote`, `Propose`. |
| `plutus_version` | `PlutusVersion` | `ScriptContext::script_lang` (Plutus arm; native arm filtered out before this struct is constructed) | `V1` / `V2` / `V3`. |
| `ex_units_budget` | `ExUnits` | `ScriptContext::redeemer.ex_units` | Declared budget the redeemer requested. |
| `ex_units_used` | `ExUnits` | UPLC machine result (mem/cpu actually spent during evaluation) | Reads the consumed-budget side of `program.eval_with_params(...)`. |
| `is_success` | `bool` | derived from per-version success rule already implemented in `evaluate_single_script` | True iff the per-version success check passed. |
| `error_message` | `Option<String>` | derived from `Phase2ValidationError` produced by the failure path | `None` on success; on failure, a short human-readable string (the existing `UplcMachineError::ScriptFailed { message, .. }` text or the rendered `Phase2ValidationError`). Capped at 512 chars at construction time. |

**Derives**: `Debug`, `Clone`, `serde::Serialize`, `serde::Deserialize`. (No `minicbor` derive — this type does not flow into chain CBOR; only Caryatid in-memory + JSON.)

**Validation rules**:
- `error_message` is `None` ⇔ `is_success == true`.
- `ex_units_used.mem ≤ ex_units_budget.mem` and `ex_units_used.steps ≤ ex_units_budget.steps` whenever `is_success == true` (a successful script must have respected its budget).

---

## 2. `Phase2EvaluationResultsMessage`  *(new, `common/src/messages.rs`)*

The unit published on `cardano.utxo.phase2`. One message per transaction that ran phase-2 (i.e., had at least one redeemer post-Alonzo).

| Field | Type | Notes |
|---|---|---|
| `tx_hash` | `TxHash` | The transaction whose scripts were evaluated. |
| `tx_index_in_block` | `u32` | Position of the tx within its block. |
| `is_valid` | `bool` | Mirrors `TxUTxODeltas::is_valid`; tells the consumer whether the tx was *expected* to succeed (false → scripts expected to fail per Alonzo spec). |
| `outcomes` | `Vec<ScriptEvaluationOutcome>` | One element per Plutus script context evaluated for this tx. May be empty if all scripts were native (in which case the message is skipped). |

**Derives**: `Debug`, `Clone`, `serde::Serialize`, `serde::Deserialize`.

**Variant addition**:
```rust
// in CardanoMessage enum
Phase2EvaluationResults(Phase2EvaluationResultsMessage),
```

The standard Caryatid pattern routes this together with the per-block `BlockInfo` already wrapped at the publish site, so subscribers receive `(BlockInfo, Phase2EvaluationResultsMessage)` — which is how the visualizer obtains epoch and block number/hash without needing them inside the message itself.

**Validation rules**:
- `outcomes.is_empty()` ⇒ message MUST NOT be published. (Skip native-only txs.)
- The publisher publishes the message only when the `publish-phase2-results` flag is true; this is enforced at the call site in `utxo_state.rs::run`.

---

## 3. `ScriptEvalEvent`  *(new, internal to `modules/script_eval_visualizer/src/stream.rs`)*

The unit pushed over the broadcast channel and serialized to each connected SSE client. One per script (i.e., the visualizer fans out a single `Phase2EvaluationResultsMessage` carrying *N* outcomes into *N* `ScriptEvalEvent`s).

| Field | Type | JSON name | Source |
|---|---|---|---|
| `id` | `u64` | `id` | Monotonically incrementing per-process counter assigned at fan-out; used as the SSE `id:` field for client-side dedup. |
| `epoch` | `u64` | `epoch` | `BlockInfo.epoch` |
| `slot` | `u64` | `slot` | `BlockInfo.slot` |
| `block_number` | `u64` | `blockNumber` | `BlockInfo.number` |
| `block_hash` | `String` (lowercase hex) | `blockHash` | `BlockInfo.hash` |
| `tx_hash` | `String` (lowercase hex) | `txHash` | `Phase2EvaluationResultsMessage.tx_hash` |
| `script_hash` | `String` (lowercase hex) | `scriptHash` | `ScriptEvaluationOutcome.script_hash` |
| `purpose` | `String` | `purpose` | `ScriptEvaluationOutcome.purpose` rendered as one of `"spend"`, `"mint"`, `"cert"`, `"reward"`, `"vote"`, `"propose"` |
| `plutus_version` | `String` | `plutusVersion` | `"v1"` / `"v2"` / `"v3"` |
| `mem` | `u64` | `mem` | `ScriptEvaluationOutcome.ex_units_used.mem` |
| `cpu` | `u64` | `cpu` | `ScriptEvaluationOutcome.ex_units_used.steps` |
| `mem_budget` | `u64` | `memBudget` | `ScriptEvaluationOutcome.ex_units_budget.mem` |
| `cpu_budget` | `u64` | `cpuBudget` | `ScriptEvaluationOutcome.ex_units_budget.steps` |
| `success` | `bool` | `success` | `ScriptEvaluationOutcome.is_success` |
| `error` | `Option<String>` | `error` (omitted if null) | `ScriptEvaluationOutcome.error_message` |

**Derives**: `Debug`, `Clone`, `serde::Serialize`.

The SSE `event:` field is fixed to `"script_eval"` for these. The single startup event is `event: "init"` and carries `{ "cexplorerBaseUrl": "...", "network": "mainnet" }`.

---

## State transitions

The data is one-directional and stateless on the publisher side:

```text
TxUTxODeltas + ScriptContexts
        │
        ▼  (in-process, evaluate_scripts)
Vec<ScriptEvaluationOutcome>     ── carried up to utxo_state.rs ──▶
        │
        ▼  (only if publish-phase2-results = true)
Phase2EvaluationResultsMessage   ── published on cardano.utxo.phase2 ──▶
        │
        ▼  (visualizer module subscription)
broadcast::Sender<ScriptEvalEvent>
        │   (fan-out: one event per outcome)
        ▼
SSE clients (each gets a BroadcastStream<ScriptEvalEvent>)
        │
        ▼  (browser)
table state (deque of ≤1000, newest first)
```

No persistence. No back-edges. Browser-side state lives only as long as the page.
