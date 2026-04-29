# Quickstart: Script Evaluation Visualizer

## Goal

Run an Acropolis omnibus node locally with phase-2 evaluation publishing turned on, open the visualizer in a browser, and confirm rows arrive as Plutus transactions are evaluated.

## Prerequisites

- A working Acropolis dev environment (`make build` succeeds on `main`).
- An `omnibus.toml` configured for a network that actually has Plutus traffic (`mainnet`, `preprod`, or `preview` — local genesis won't show anything).

## Steps

### 1. Enable publishing in `utxo_state`

In `processes/omnibus/omnibus.toml`, under `[module.utxo-state]`, add:

```toml
publish-phase2-results = true
publish-phase2-topic   = "cardano.utxo.phase2"   # default; can be omitted
```

Without this, the publisher takes a single-cmp early-out and the visualizer page will load but stay empty.

### 2. Configure the visualizer module

In `processes/omnibus/omnibus.toml`, add (or confirm):

```toml
[module.script-eval-visualizer]
phase2-subscribe-topic = "cardano.utxo.phase2"   # must match step 1
bind-address           = "127.0.0.1"
bind-port              = 8030
```

### 3. Run the node

```bash
make run                    # or:  make run-bootstrap   (snapshot start, faster)
```

Wait until logs show `cardano.utxo.deltas` traffic (i.e., the node is processing blocks).

### 4. Open the visualizer

Open `http://127.0.0.1:8030/` in a browser. The page should:

1. Show an empty table titled "Script evaluations" with column headers (Epoch, Block, Tx, Script, Purpose, Plutus, Mem, CPU, Status).
2. Connect to `/events` (visible in the network tab as a long-lived `text/event-stream` response).
3. Begin filling rows from the top as the node processes blocks containing Plutus transactions.

### 5. Verify acceptance criteria

- **FR-006 / SC-003**: every row shows all nine fields.
- **FR-008 / SC-004**: clicking the block-number cell opens a new tab to the corresponding cexplorer.io page; same for the tx-hash cell.
- **FR-009**: newest evaluation is at the top.
- **FR-010 / SC-002**: leave the page open through > 1000 evaluations and confirm the table caps at 1000 rows.
- **FR-007**: trigger a deliberately-failing transaction (e.g., on preview with a known always-fail script) and confirm the row is visually distinguishable from successful rows.
- **FR-002 / SC-005**: set `publish-phase2-results = false`, restart the node, reload the page, and confirm no new rows arrive.

## Smoke test (CI / dev loop)

```bash
cargo test -p acropolis_module_script_eval_visualizer            # unit + crate-local tests
cargo test -p acropolis_module_utxo_state                        # ensures phase-2 refactor still passes
cargo test --test stream_integration -p acropolis_module_script_eval_visualizer
                                                                  # in-process pub→fan-out→broadcast
make all                                                         # fmt + clippy + test
```

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| Page loads but no rows ever appear | `publish-phase2-results = false` | Step 1. |
| Page loads but no rows after enabling | Topic mismatch between publisher and subscriber | Make `publish-phase2-topic` and `phase2-subscribe-topic` identical. |
| Connection refused on port 8030 | Bind port collision | Change `bind-port`; verify `lsof -iTCP:8030`. |
| `lagged` events appear in dev tools | Slow client or burst | Expected behaviour; the bus continues. Increase broadcast capacity in `stream.rs` if persistent. |
| No Plutus traffic at all | Local genesis or bootstrap network has no Plutus | Switch to a real network (preprod/preview/mainnet). |
