# UXTO state

The UTXO state module accepts UTXO changes and maintains an
in-memory UTXO state.  It naively tracks the creation and spending
of UTXOs and logs them.  It doesn't currently have any query interface or
generate any further messages.

Note it does not yet hold enough state to handle rollbacks.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.utxo-state]

# Message topics
subscribe-topic = "cardano.utxo.deltas"
pool-certificates-deltas-subscribe-topic = "cardano.pool.certificates.deltas"
stake-certificates-deltas-subscribe-topic = "cardano.stake.certificates.deltas"
```

## Messages

The utxo state module subscribes for UTXODeltasMessages `cardano.utxo.deltas`
(see the [Transaction Unpacker](../tx_unpacker) module for details).

It doesn't currently publish any messages.

Also subscribe to `StakeRegistrationUpdates` Messages and `PoolRegistrationUpdates` Messages
in order to validate UTxO Rule `ValueNotConservedUTxO` because we need to 
calculate transaction's `deposit` and `refund` for Stake Address & Pool's Registration

## Phase-2 evaluation publishing (optional)

For operator-facing visibility into Plutus phase-2 script evaluations, this
module can optionally publish a per-transaction `Phase2EvaluationResultsMessage`
on a dedicated topic. The feature is **disabled by default** — operators opt in
by adding the following to `[module.utxo-state]`:

```toml
publish-phase2-results = true
publish-phase2-topic   = "cardano.utxo.phase2"   # default; can be omitted
```

When the flag is `false` (or absent), no message is constructed and no
publication is attempted — the validation hot path is unchanged.

The companion module
[`acropolis_module_script_eval_visualizer`](../script_eval_visualizer) consumes
this topic and serves a live HTML+SSE table of recent evaluations.
