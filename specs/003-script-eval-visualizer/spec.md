# Feature Specification: Script Evaluation Visualizer

**Feature Branch**: `003-script-eval-visualizer`
**Created**: 2026-04-29
**Status**: Draft
**Input**: User description: "Build a caryatid module under modules folder. This module will listen to utxo_state's event. utxo_state will publish phase 2 validation result on that channel. (cardano.utxo.phase2) containing information of transaction, script (being evaluated), and the result. The result is per transaction, which can be retrieved from evaluate_scripts function. And the result will contain array of sub result of each script evaluation. The new module (script evaluation visualizer module) will listen to this event, and show scripts evaluation in real time in frontend (html with simple react state.) The frontend will show the table, where the recently evaluated script (with additional informations: epoch number, block number, transaction hash, script hash, script purpose, plutus version, mem and cpu amount, block and transaction hash will be a href which opens new tab to cexplorer.io with block number or tx hash). The frontend will only show the latest 1000 scripts evaluations. (the old ones will get removed, by adding new ones to the top of the table). The frontend and visualizer module will talk using server sent event. And this feature (publishing scripts evaluation results in utxo_state) can be turn on/off."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Live monitoring of phase-2 script evaluations (Priority: P1)

A node operator or protocol engineer running an Acropolis node opens a browser page served by the visualizer and immediately begins seeing each Plutus script evaluation that the node performs as it processes new blocks. Every row contains the per-script execution metadata they need to spot anomalies (unexpected failures, oversized budgets, unusual purposes) without leaving the page or running ad-hoc log greps.

**Why this priority**: This is the core value of the feature. Without it, none of the other behaviors matter — the entire purpose is to surface phase-2 evaluation results to a human in real time.

**Independent Test**: Run the node with the publishing feature enabled, open the visualizer page in a browser, and confirm that rows appear (newest at the top) as the node validates blocks containing transactions with Plutus scripts, with the most recent evaluation visible without manual refresh.

**Acceptance Scenarios**:

1. **Given** the node is running with phase-2 publishing enabled and the visualizer page is open, **When** the node validates a transaction containing one Plutus script, **Then** a new row appears at the top of the table within a few seconds showing the script's epoch, block number, transaction hash, script hash, purpose, Plutus version, memory units, and CPU units.
2. **Given** the node validates a transaction containing multiple Plutus scripts, **When** the evaluations complete, **Then** one row per script appears in the table (each with its own script hash, purpose, and execution units), all referencing the same transaction hash.
3. **Given** a script evaluation fails phase-2 validation, **When** the row is rendered, **Then** the failure is visually distinguishable from a successful evaluation (e.g., a status column or color cue) so the operator can spot it at a glance.
4. **Given** the table currently displays 1000 rows, **When** a new evaluation arrives, **Then** the new row appears at the top and the oldest row is removed, keeping the displayed total at 1000.

---

### User Story 2 - Drill into a transaction or block via cexplorer.io (Priority: P2)

The operator sees a suspicious or interesting evaluation in the table and wants to inspect the surrounding on-chain context (full transaction body, block contents, neighboring transactions). They click the block number or transaction hash in the row and a new browser tab opens directly to that item on cexplorer.io.

**Why this priority**: This dramatically reduces investigation friction but is a navigational convenience layered on top of the core monitoring view; the table is still useful without it.

**Independent Test**: With at least one evaluation visible in the table, click the block-number link and verify a new tab opens to the cexplorer.io page for that block; click the transaction-hash link and verify a new tab opens to the cexplorer.io page for that transaction.

**Acceptance Scenarios**:

1. **Given** a row is visible in the table, **When** the user clicks the block-number cell, **Then** a new browser tab opens to the cexplorer.io page for that block on the network the node is connected to.
2. **Given** a row is visible in the table, **When** the user clicks the transaction-hash cell, **Then** a new browser tab opens to the cexplorer.io page for that transaction.
3. **Given** the user clicks a link, **When** the new tab opens, **Then** the visualizer page itself remains open and continues receiving live updates.

---

### User Story 3 - Toggle phase-2 result publishing on and off (Priority: P2)

An operator running the node in production or under heavy load wants the freedom to disable the publication of phase-2 evaluation results so that the node does not pay the cost of emitting them when nobody is watching, and to re-enable them when debugging or during normal monitoring.

**Why this priority**: Operationally important to avoid imposing a cost on workloads that don't need the visibility, but the visualizer itself remains demonstrable and testable without this toggle in place.

**Independent Test**: With the visualizer open, disable phase-2 result publishing in the node configuration; restart or otherwise apply the change; observe that no new rows arrive even as the node processes new blocks. Re-enable publishing; observe that rows resume appearing.

**Acceptance Scenarios**:

1. **Given** phase-2 publishing is disabled in the node configuration, **When** the node validates transactions with Plutus scripts, **Then** the visualizer displays no new rows and the node performs no work that would otherwise be required only to publish phase-2 results.
2. **Given** phase-2 publishing is enabled, **When** the operator views the visualizer, **Then** evaluations appear as in User Story 1.
3. **Given** the configuration's default value, **When** the node starts without explicit configuration of this toggle, **Then** the chosen default behavior (see Assumptions) is applied predictably and is documented.

---

### Edge Cases

- **No evaluations yet**: When the visualizer page is first opened and the node has not yet processed any phase-2 evaluations, the table displays an empty state (or a brief "waiting for evaluations…" message) rather than appearing broken.
- **Connection loss / page reload**: If the SSE connection drops or the user reloads the page, the table starts fresh from that moment forward; historical evaluations from before the reconnection are not replayed (the feature is for live monitoring, not historical querying).
- **Multiple browser tabs / clients**: Multiple simultaneous viewers each receive the same live stream; one client's reload does not affect others.
- **Burst of evaluations**: A single block can contain many transactions, each with multiple scripts. The page must remain usable (table still scrollable, links still clickable) even when many rows arrive in quick succession.
- **Rollbacks**: If the node rolls back blocks whose script evaluations have already been displayed, those rows are not retroactively removed (the table shows what the node *did evaluate*, not what is necessarily still on the canonical chain). This behavior is documented so operators are not confused.
- **Non-Plutus / native scripts**: Native scripts and other items that do not go through phase-2 Plutus evaluation are not displayed; only phase-2 Plutus script evaluations appear.
- **Feature disabled at runtime**: When publishing is disabled while the visualizer page is open, no new rows arrive but existing rows remain visible until the page is reloaded.
- **Cexplorer.io network mismatch**: The cexplorer.io link uses the network the node is connected to (mainnet vs. preprod/preview); links for a non-mainnet network point to the appropriate cexplorer.io subdomain or are documented as best-effort.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The node MUST be able to publish, on a dedicated message channel (`cardano.utxo.phase2`), a per-transaction phase-2 validation result that contains the transaction context, the set of scripts evaluated for that transaction, and the per-script evaluation outcome (success/failure plus execution units and any error context).
- **FR-002**: The publication described in FR-001 MUST be controllable via a configuration flag, allowing operators to turn the feature on or off without code changes; when off, the node MUST NOT emit messages on this channel and MUST NOT incur the costs that exist solely to support that emission.
- **FR-003**: A new module dedicated to visualization MUST subscribe to the `cardano.utxo.phase2` channel and convert each incoming per-transaction result into one downstream record per individual script evaluation contained in the result.
- **FR-004**: The visualizer module MUST serve a web page that, when opened in a modern browser, renders a table of recent script evaluations and updates that table in real time as new evaluations arrive from the node, without requiring manual page refresh.
- **FR-005**: The push of evaluations from the visualizer module to the browser MUST use a Server-Sent-Events stream (a persistent one-way HTTP connection from server to client).
- **FR-006**: Each row in the table MUST display, at minimum: epoch number, block number, transaction hash, script hash, script purpose (e.g., spending, minting, certifying, rewarding, voting, proposing), Plutus language version, memory units consumed, and CPU units consumed.
- **FR-007**: Each row MUST visually indicate whether the script evaluation succeeded or failed, so an operator scanning the table can distinguish failures from successes without reading individual cells.
- **FR-008**: The block-number cell and the transaction-hash cell in each row MUST be hyperlinks; clicking the block-number link MUST open a new browser tab to the cexplorer.io page for that block, and clicking the transaction-hash link MUST open a new browser tab to the cexplorer.io page for that transaction.
- **FR-009**: The table MUST display newest evaluations at the top; new arrivals MUST be inserted at the top of the table.
- **FR-010**: The table MUST cap its visible content at the most recent 1000 evaluations; when a new evaluation arrives while the cap is reached, the oldest evaluation MUST be removed from the table so the displayed total stays at 1000.
- **FR-011**: The visualizer MUST render an empty state when no evaluations have been received yet, rather than appearing broken or blank without explanation.
- **FR-012**: The visualizer MUST tolerate transient SSE disconnections gracefully (e.g., the page does not crash, and reconnect attempts may be made) but is NOT required to replay evaluations that occurred while disconnected.
- **FR-013**: When phase-2 publishing is disabled in node configuration, opening the visualizer page MUST still succeed and MUST clearly behave as expected (i.e., empty table, no errors); the page does not need to detect or display the disabled state.
- **FR-014**: The visualizer module MUST cope with bursts of evaluations (a single block can contain many script-bearing transactions) without dropping the connection to the browser and without making the page unresponsive at the displayed cap of 1000 rows.

### Key Entities *(include if feature involves data)*

- **Phase-2 Validation Result (per transaction)**: Represents the outcome of phase-2 script validation for a single transaction. Carries the transaction's context (epoch, block, transaction identity) and an ordered collection of per-script sub-results. This is the unit published on the `cardano.utxo.phase2` channel.
- **Script Evaluation Record (per script)**: One element within the per-transaction result, representing a single Plutus script execution. Attributes: script hash, script purpose, Plutus language version, memory units consumed, CPU units consumed, success/failure outcome, and (on failure) any error context provided by the evaluator.
- **Visualizer Stream Event**: The unit pushed over SSE to the browser. Each event corresponds to one Script Evaluation Record enriched with the surrounding transaction context (epoch, block number, transaction hash) so the frontend can render a complete row without needing additional lookups.

## Assumptions

- **Default for the publishing toggle**: The publishing of phase-2 results is **disabled by default**, since the feature is intended primarily for operator monitoring and debugging, and the cost of emitting evaluation results should not be paid by every node by default. Operators opt in via configuration. This default is confirmable/changeable during planning.
- **Cexplorer.io URL scheme**: Block links use the standard cexplorer.io path for blocks (by block number) and transaction links use the standard path for transactions (by transaction hash). The exact subdomain/path is the conventional one used by the wider Cardano community; for non-mainnet networks (preprod/preview), the corresponding cexplorer.io subdomain is used; if no equivalent exists for a given network, links may point at mainnet as a documented limitation.
- **No historical replay**: The visualizer is a *live* monitor. It does not persist evaluations to disk and does not replay history when a client connects mid-stream; clients see only what arrives after they connect.
- **Authentication / access control**: The HTTP endpoint serving the page and SSE stream is intended for local-operator use (same-host or trusted network). No authentication is built in for this initial version; deployment in untrusted environments is out of scope.
- **Frontend technology shape**: The frontend is "simple React state" served as static HTML/JS by the visualizer module — no build tooling, separate deployment, or external CDN dependency is required for the operator to use the feature.
- **Rollback handling**: Rolled-back evaluations are not retroactively removed from the displayed table; the table reflects what was evaluated, which is acceptable for a live monitoring use case.
- **Non-Plutus scripts**: Only phase-2 Plutus evaluations are surfaced; native (Timelock) scripts and other constructs that do not undergo phase-2 evaluation are out of scope.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: When phase-2 publishing is enabled and the node is processing blocks containing Plutus transactions, an operator who opens the visualizer page sees the first new evaluation row appear within 3 seconds of the node finishing that evaluation, under nominal load.
- **SC-002**: At steady state with the displayed cap reached, the table consistently shows exactly 1000 rows; verified by 100% of test cases that drive more than 1000 evaluations through the system.
- **SC-003**: 100% of rows include all nine required fields (epoch, block number, transaction hash, script hash, purpose, Plutus version, memory units, CPU units, success/failure indicator) — no row is rendered with a missing required field.
- **SC-004**: 100% of clicks on a block-number or transaction-hash link in a rendered row open a new browser tab to the corresponding cexplorer.io page on the node's current network, without affecting the visualizer tab.
- **SC-005**: When the publishing toggle is off, a node running an otherwise identical workload performs no work attributable to this feature beyond what would be needed if the feature were absent (verifiable by the absence of messages on the channel and the absence of evaluation-emission code paths firing).
- **SC-006**: The visualizer page remains responsive (interactive UI; links still clickable; new rows still arrive) for at least 1 hour of continuous operation receiving evaluations at the rate produced by mainnet block traffic.
- **SC-007**: An operator unfamiliar with the codebase can enable the toggle, open the page, and successfully identify a failed script evaluation from a deliberately broken transaction in under 5 minutes.
