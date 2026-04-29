# Contract: `cardano.utxo.phase2` Caryatid message

## Channel

| Property | Value |
|---|---|
| Topic (default) | `cardano.utxo.phase2` |
| Topic config key (publisher) | `publish-phase2-topic` (under `[module.utxo-state]`) |
| Topic config key (subscriber) | `phase2-subscribe-topic` (under `[module.script-eval-visualizer]`) |
| Direction | `utxo_state` → `script_eval_visualizer` (and any other future subscriber) |
| Cardinality | At most one message per transaction; only emitted when at least one Plutus script was evaluated for that tx and `publish-phase2-results = true`. |
| Ordering | In transaction order within a block; in block order across blocks (matches the upstream `cardano.utxo.deltas` order). |
| Rollback semantics | Evaluation events are not retracted on rollback. (Per L001, this is a deliberate decision — see spec assumptions and FR-014.) |

## Payload

Wrapped in the standard Caryatid `Message::Cardano((BlockInfo, CardanoMessage::Phase2EvaluationResults(Phase2EvaluationResultsMessage)))`.

```rust
/// Per-transaction phase-2 Plutus script validation results.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Phase2EvaluationResultsMessage {
    /// Hash of the transaction these outcomes belong to.
    pub tx_hash: TxHash,

    /// Position of the transaction within its block (0-indexed).
    pub tx_index_in_block: u32,

    /// Whether the tx is marked valid (false ⇒ Alonzo "expected to fail").
    pub is_valid: bool,

    /// One element per Plutus script that ran. Native scripts are not represented.
    pub outcomes: Vec<ScriptEvaluationOutcome>,
}

/// One Plutus script evaluation result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptEvaluationOutcome {
    pub script_hash: ScriptHash,
    pub purpose: RedeemerTag,
    pub plutus_version: PlutusVersion,
    pub ex_units_budget: ExUnits,
    pub ex_units_used: ExUnits,
    pub is_success: bool,

    /// `None` on success. On failure, a short rendered error (≤ 512 chars).
    pub error_message: Option<String>,
}
```

## Publisher invariants

1. The publisher MUST NOT emit a `Phase2EvaluationResultsMessage` whose `outcomes` is empty.
2. The publisher MUST NOT do any work attributable to this message (constructing `ScriptEvaluationOutcome`s, allocating the `Vec`, calling `publish`) when `publish-phase2-results = false`.
3. The publisher MUST emit messages in chain-application order. Reorder is not allowed.

## Subscriber expectations

1. Subscribers MUST tolerate occasional gaps (e.g., on transient bus errors).
2. Subscribers MUST NOT mutate the value (Caryatid uses `Arc`-shared messages).
3. Subscribers MAY drop messages for slow clients; doing so MUST NOT block the publisher (the visualizer's broadcast channel is `tokio::sync::broadcast`, which yields `Lagged(n)` rather than blocking).
