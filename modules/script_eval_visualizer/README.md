# Script Evaluation Visualizer

A Caryatid module that serves a live HTML+SSE table of phase-2 Plutus script
evaluations performed by the running node. Intended for operator-facing
debugging and monitoring.

## What it does

- Subscribes to `cardano.utxo.phase2` (configurable), the topic on which
  [`utxo_state`](../utxo_state) publishes per-transaction phase-2 evaluation
  results when the operator opts in.
- Fans each transaction's per-script outcomes into individual events.
- Serves a small embedded HTML page (`GET /`) and a Server-Sent-Events stream
  (`GET /events`) that pushes those events to the browser in real time.
- The page renders the most recent 1000 evaluations, newest at the top, with
  cexplorer.io drill-in links for the block hash and transaction hash.

## Endpoints

| Method | Path        | Purpose                                        |
|--------|-------------|------------------------------------------------|
| GET    | `/`         | Embedded HTML page (vanilla JS, no build step) |
| GET    | `/events`   | SSE stream (`text/event-stream`)               |
| GET    | `/healthz`  | Liveness probe (`200 ok`)                      |

## Configuration

```toml
[module.script-eval-visualizer]
phase2-subscribe-topic = "cardano.utxo.phase2"   # must match utxo-state's publish topic
bind-address           = "127.0.0.1"             # loopback by default — operator-local
bind-port              = 8030
network                = "mainnet"               # selects cexplorer.io subdomain
```

The defaults shown above apply when keys are omitted. The bind address is
loopback by default; running this on a non-trusted network requires a reverse
proxy that adds authentication.

## Dependency on `utxo_state`

This module is a passive consumer. It receives nothing unless `utxo_state` is
configured to publish:

```toml
[module.utxo-state]
publish-phase2-results = true
```

When that flag is `false` (the default), the visualizer page still loads but
the table stays empty. See [`utxo_state` README](../utxo_state/README.md) for
the publishing-side toggle.

## Design notes

- The visualizer keeps no history. It is a *live* monitor: connecting clients
  see only events that arrive after they connect; reloads start fresh.
- Rolled-back blocks' evaluations are not retroactively removed. The table
  shows what the node *did evaluate*, not what is currently on the canonical
  chain.
- Backpressure: the in-process broadcast channel has a fixed capacity; slow
  clients receive a `lagged` event indicating how many events were dropped for
  them, while the bus continues feeding fast clients.
