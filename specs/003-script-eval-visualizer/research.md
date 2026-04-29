# Phase 0 Research: Script Evaluation Visualizer

## Open questions resolved

### Q1. How should the new module deliver HTTP + SSE? Reuse the in-tree `caryatid_module_rest_server` or run an embedded `axum` server?

**Decision**: Run an embedded `axum` server inside the module, on its own port. Same pattern as `modules/mcp_server/src/server.rs` (which constructs its own `axum::Router` + `TcpListener` rather than registering with the shared REST module).

**Rationale**:
- The visualizer is operationally distinct from the Blockfrost REST surface and benefits from being independently start/stoppable on its own port.
- The shared `caryatid_module_rest_server` is request/response-shaped; SSE requires owning the response stream and lifetime, which is more naturally expressed by holding a direct `axum::routing::get` handler.
- Closest in-tree precedent (`mcp_server`) does exactly this and shows the pattern works well alongside other modules.

**Alternatives considered**:
- *Use `caryatid_module_rest_server` and emit SSE events as long-lived response chunks*: rejected — the shared server abstracts request/response into `RESTRequest`/`RESTResponse` messages; streaming bodies would require new infrastructure that the rest of the workspace doesn't need.
- *Plain `hyper`*: rejected — `axum` is already in workspace deps and is the established higher-level choice in the project.

---

### Q2. How should per-script outcomes be surfaced from `evaluate_scripts` without disturbing existing callers?

**Decision**: Refactor `evaluate_scripts` to internally collect a `Vec<ScriptEvaluationOutcome>` and return it alongside the existing aggregate result. The public signature changes from
```
pub fn evaluate_scripts(...) -> Result<(), Phase2ValidationError>
```
to
```
pub fn evaluate_scripts(...) -> (Vec<ScriptEvaluationOutcome>, Result<(), Phase2ValidationError>)
```
where each `ScriptEvaluationOutcome` records: `script_hash`, `script_purpose: RedeemerTag`, `plutus_version: PlutusVersion`, declared `ex_units_budget`, observed `ex_units_used`, `is_success`, and (on failure) a short error message.

The existing single-call site `validations::phase_two::mod.rs::validate_tx_phase_two` adapts: it returns the outcomes vec to its caller (`validations::mod.rs::validate_tx`), which threads it up to `state.rs::validate`, which finally returns it to `utxo_state.rs::run`, where the publisher emits the message **only if** the publish-phase2-results flag is enabled.

**Rationale**:
- Minimal blast radius: `evaluate_scripts` already iterates per-script via `par_iter`. Each iteration produces a `Result` we currently `try_for_each` over; we collect them all instead.
- The existing aggregate-error semantics are preserved bit-for-bit; nothing else in the codebase changes behaviour.
- Per-script `ex_units_used`: the UPLC machine's `eval_with_params` returns a result that includes consumed budget — verified at `evaluate.rs:383` (`result = program.eval_with_params(...)`); the `result` value carries `mem`/`cpu` deltas alongside `term`. Wiring those into `ScriptEvaluationOutcome` is bookkeeping, not new logic.

**Alternatives considered**:
- *Tee the per-script outcomes via a side channel (mpsc) inside `evaluate_scripts`*: rejected — adds a hidden coupling between a pure validation function and the publisher; harder to reason about for tests.
- *Wrap `Phase2ValidationError` to carry per-script context*: rejected — error types should describe failures, not successes; success mem/cpu numbers don't belong on an error type.
- *Re-run evaluation a second time only when the publish flag is on*: rejected — doubles validation cost on the very workload that opted in to monitoring; defeats the point.

---

### Q3. Where does the per-script outcome get enriched with block context (epoch, block number, transaction hash)?

**Decision**: At publication time inside `utxo_state.rs::run`, where `BlockInfo` and the per-tx `TxUTxODeltas` are both already in scope. The `Phase2EvaluationResultsMessage` carries the enriched fields directly so neither downstream consumers nor the visualizer module have to look anything up.

**Rationale**:
- `validate_tx` is per-tx and would have to be re-given `BlockInfo` to enrich; cleaner to pass raw outcomes up and stamp context once at the top.
- The visualizer module does not import `acropolis_common::TxUTxODeltas` or any UTXO state — keeping its dep graph narrow.

**Alternatives considered**:
- Enriching in the visualizer module: rejected — would force the visualizer to subscribe to `cardano.utxo.deltas` and `cardano.protocol.parameters` just to get epoch/block, violating its single-purpose nature.

---

### Q4. Frontend: real React or hand-rolled "React-like state"?

**Decision**: Hand-rolled vanilla JS using a tiny `setState`-style pattern (one module-level `state` object, one `render()` that rebuilds the table from `state`, one `applyEvent(e)` that prepends + truncates to 1000). No React, no JSX, no build step. Embedded as a single `index.html` via `include_str!`.

**Rationale**:
- The user's "simple React state" wording is about the *shape* of the UI logic (a list managed by state, declarative re-render), not a hard requirement on the React library. A 50-line vanilla implementation is faster, has zero supply-chain surface, and respects the "no build tooling" constraint.
- Avoiding a CDN avoids network failure modes when operators run in air-gapped or restricted environments.
- A 1000-row table is well within reach of plain DOM; no virtualization or framework needed.

**Alternatives considered**:
- *React via CDN (`react.production.min.js` + `react-dom.production.min.js` + `htm` for JSX-free templates)*: rejected — adds external dependencies to a tool whose whole point is local visibility; brittle in restricted networks.
- *Bundle React with a build step (esbuild/vite)*: rejected — introduces a JS toolchain into a Rust project, doubles CI complexity for a tiny UI.
- *Server-rendered HTML re-fetched on a timer*: rejected — would defeat the SSE requirement (FR-005) and the "real time" spec language.

---

### Q5. Cexplorer.io URL scheme per network.

**Decision**: Map the `BlockInfo.network` (already known to `utxo_state` and the omnibus config) to a base URL:
- mainnet → `https://cexplorer.io`
- preprod → `https://preprod.cexplorer.io`
- preview → `https://preview.cexplorer.io`
- sancho / others → fall back to `https://cexplorer.io` (best-effort, documented).

Block path: `/block/{block_hash}` (cexplorer accepts hash on its block detail route — more reliable than block-number disambiguation across forks). Tx path: `/tx/{tx_hash}`.

The base URL is included once in the SSE `init` event (the very first event the server sends to a newly connected client) so the client can construct links without re-deriving from rows.

**Rationale**:
- Block hash is stable and unambiguous; block number alone is ambiguous during reorgs (and the spec already states rolled-back rows are not retroactively removed).
- Sending the base URL once as `init` keeps each row's payload small and avoids embedding the network into every row.

**Alternatives considered**:
- *Embed the full URL in each row*: rejected — wastes SSE bandwidth and couples the server to every URL change.
- *Link by block number*: documented as the user's verbatim ask; we still include the block number in the row text for human readability but use block hash for the href, since that is what cexplorer canonically uses for direct block deep-links.

---

### Q6. SSE resume / `Last-Event-ID`?

**Decision**: Do **not** support resume. Per spec FR-012, the visualizer is not required to replay history. Each event still carries an incrementing `id:` field for client-side dedup if the user opens two tabs, but the server keeps no per-id history.

**Rationale**: Live monitor; persisting evaluations is explicitly out of scope (see spec assumptions). Avoids storage and bounds memory in the module.

---

### Q7. Backpressure / slow client behaviour.

**Decision**: One `tokio::sync::broadcast::Sender<ScriptEvalEvent>` (capacity 4096) is owned by the module. Each Caryatid subscription handler does `tx.send(event).ok();` (drop on error — i.e., when there are zero subscribers). Each connected SSE client wraps its receiver in `BroadcastStream` and serializes to SSE; if the client lags by more than 4096 events `BroadcastStream` yields `Err(Lagged(n))`, which the SSE handler converts into a single `lagged: n` SSE comment and continues. The bus is never blocked by a slow browser tab.

**Rationale**: Standard `tokio::sync::broadcast` semantics. Capacity sized to fit a worst-case mainnet block (low thousands of script evaluations) without dropping any client.

---

### Q8. Default port and address.

**Decision**: Defaults — `bind-address = "127.0.0.1"`, `bind-port = 8030`. Operator-overridable via config. Loopback-only by default to satisfy the "operator-local" assumption in the spec without anyone having to read the docs.

**Rationale**: Matches in-tree precedent for development-time HTTP services; avoids accidental network exposure of an unauthenticated endpoint. Port 8030 chosen to sit clear of in-tree REST/MCP ports (which use 3000-range and 4000-range respectively in default omnibus.toml).

---

### Q9. Default value of `publish-phase2-results` flag in `utxo_state`.

**Decision**: `publish-phase2-results = false` by default in `omnibus.toml` (and absent → false in code). Operators opt in.

**Rationale**: Aligns with the spec's "Assumptions" section, satisfies SC-005 (zero cost when off), and means the feature ships dark — operators turn it on intentionally.

---

## Best-practices notes

- **L001 / rollback semantics**: explicit non-rescind behaviour for evaluation events documented in spec + plan; no withdraw-on-rollback message.
- **L007 / external CLI tools**: not applicable — this feature has no shell-script entry points.
- **L009 / shell-injection in `eval`**: not applicable — no shell `eval` involved.
- **No `unwrap`/`panic`**: enforced by clippy + manual review; SSE handler `?`-propagates serialization errors but logs-and-continues per-event so one bad event does not kill a long-lived stream.
- **Doc comments on public items**: all module-level public types (`Phase2EvaluationResultsMessage`, `ScriptEvaluationOutcome`, `ScriptEvalEvent`) receive `///` doc comments per constitution rule 4.

## Resolved → no `[NEEDS CLARIFICATION]` markers remain.
