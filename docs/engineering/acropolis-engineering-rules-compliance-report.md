# Acropolis Rust Engineering Rules — Compliance Report

**Date:** 2026-02-19
**Branch:** `Implement-chain-selection-logic`
**Rules source:** `docs/engineering/acropolis-rust-engineering-rules.md`

---

## Executive Summary

| Area | Rules | Status |
|---|---|---|
| General Design | ACR-001–008 | Mostly compliant; minor ACR-006 concern |
| Layout & Complexity | ACR-009–014 | ❌ Multiple `shall`/`will` violations |
| Safety & Correctness | ACR-015–020 | ACR-015 ✅; ❌ ACR-016 widespread; ACR-019 gap |
| Concurrency & Async | ACR-021–024 | Mostly compliant; ACR-023 spot risk |
| Dependencies | ACR-025–029 | ❌ ACR-025/027/028 violations |
| Macros/Features | ACR-030–033 | ✅ Compliant |
| Error/Logging/Observability | ACR-034–037 | ACR-034/035/036 gaps; ACR-037 absent |
| Testing | ACR-038–043 | ACR-040/041/043 gaps |
| Documentation | ACR-044–046 | ❌ ACR-044 widespread; ACR-045 gap |

---

## Section 3 — General Design

### ACR-001/002/003 — Module boundaries and coupling
**Status: ✅ COMPLIANT**

All cross-module communication goes through the shared message bus using types from `common/`. No direct cross-module internal imports observed. `processes/*` crates correctly only wire modules.

### ACR-005 — Global mutable state
**Status: ✅ COMPLIANT**

No `static mut`, `lazy_static!`, or `OnceLock`-backed global mutable state found in modules or common crates.

### ACR-006 — Minimal public API surface
**Status: ⚠️ MINOR CONCERN**

`common/src/messages.rs` exposes 30+ public structs with no crate-level visibility controls. All message types are `pub` by default; consider auditing whether all are intentionally part of the public contract.

---

## Section 4 — Code Layout and Complexity

### ACR-009 (`will`) — Functions ≤ 100 LSLOC
**Status: ❌ VIOLATED**

Large functions confirmed (line spans; LSLOC will be somewhat lower but still far exceed the 100-line limit):

| File | Function | Start Line | Approx. Lines |
|---|---|---|---|
| `common/src/snapshot/streaming_snapshot.rs` | `parse` | 797 | **~707** |
| `accounts_state/src/accounts_state.rs` | `run` | 149 | **~444** |
| `accounts_state/src/accounts_state.rs` | `init` | 606 | **~399** |
| `spo_state/src/spo_state.rs` | `run` | 208 | **~374** |
| `spo_state/src/spo_state.rs` | `init` | 603 | **~435** |
| `chain_store/src/chain_store.rs` | `handle_blocks_query` | 213 | **~322** |
| `accounts_state/src/state.rs` | `enter_epoch` | 504 | **~215** |
| `common/src/snapshot/streaming_snapshot.rs` | `stream_utxos` | 1504 | **~194** |
| `drep_state/src/state.rs` | `process_one_cert` | 446 | **~149** |
| `accounts_state/src/state.rs` | `handle_tx_certificates` | 1503 | **~145** |
| `common/src/snapshot/streaming_snapshot.rs` | `parse_blocks_with_epoch` | 1720 | **~127** |
| `chain_store/src/chain_store.rs` | `init` | 55 | **~119** |
| `chain_store/src/chain_store.rs` | `handle_txs_query` | 1167 | **~114** |
| `chain_store/src/chain_store.rs` | `to_tx_info` | 747 | **~101** |

> **Note:** `streaming_snapshot.rs::parse` at ~707 lines is the single worst offender and should be the top refactoring priority.

### ACR-010 (`should`) — Files ≤ 500 LSLOC
**Status: ❌ VIOLATED** (many files)

Files significantly over 500 LSLOC (non-comment, non-blank lines):

| File | LSLOC |
|---|---|
| `common/src/types.rs` | **1948** |
| `accounts_state/src/state.rs` | **1787** |
| `common/src/snapshot/streaming_snapshot.rs` | **1602** |
| `common/src/stake_addresses.rs` | **1432** |
| `chain_store/src/chain_store.rs` | **1242** |
| `consensus/src/consensus_tree.rs` | **1083** |
| `assets_state/src/state.rs` | **1072** |
| `drep_state/src/state.rs` | **1055** |
| `common/src/address.rs` | **1003** |
| `spo_state/src/state.rs` | 972 |
| `rest_blockfrost/src/handlers/pools.rs` | 919 |
| `rest_blockfrost/src/types.rs` | 883 |
| `spo_state/src/spo_state.rs` | 867 |
| `accounts_state/src/accounts_state.rs` | 823 |
| `peer_network_interface/src/chain_state.rs` | 777 |
| `common/src/snapshot/governance.rs` | 783 |
| `rest_blockfrost/src/handlers/accounts.rs` | 759 |
| `rest_blockfrost/src/routes.rs` | 718 |
| `rest_blockfrost/src/handlers/epochs.rs` | 687 |
| `rest_blockfrost/src/handlers/addresses.rs` | 637 |
| `common/src/validation.rs` | 614 |

### ACR-011 (`shall`) — Files > 800 LSLOC must be split
**Status: ❌ VIOLATED — requires deviation documentation or immediate action**

Files exceeding the mandatory 800-LSLOC split threshold with no documented exception:

- `common/src/types.rs` (1948)
- `accounts_state/src/state.rs` (1787)
- `common/src/snapshot/streaming_snapshot.rs` (1602)
- `common/src/stake_addresses.rs` (1432)
- `chain_store/src/chain_store.rs` (1242)
- `consensus/src/consensus_tree.rs` (1083)
- `assets_state/src/state.rs` (1072)
- `drep_state/src/state.rs` (1055)
- `common/src/address.rs` (1003)
- `spo_state/src/state.rs` (972)
- `rest_blockfrost/src/handlers/pools.rs` (919)
- `rest_blockfrost/src/types.rs` (883)
- `spo_state/src/spo_state.rs` (867)
- `accounts_state/src/accounts_state.rs` (823)

> **Exception candidate:** `ledger-state/cddl-codegen/rust/src/utxos/serialization.rs` (2524 lines) is auto-generated code and qualifies for an ACR-033 exemption if documented.

### ACR-013 (`should`) — Functions ≤ 6 parameters
**Status: ✅ COMPLIANT**

No functions with more than 6 parameters confirmed. The codebase appears to use struct/context arguments appropriately for complex cases.

### ACR-014 (`shall`) — Nesting depth > 4 shall be refactored
**Status: ❌ VIOLATED**

Files with significant deep nesting (lines with 5+ indent levels, i.e. ≥ 20 leading spaces):

| File | Lines with deep nesting |
|---|---|
| `chain_store/src/chain_store.rs` | **497** |
| `spo_state/src/spo_state.rs` | **446** |
| `accounts_state/src/accounts_state.rs` | **414** |
| `accounts_state/src/state.rs` | **367** |
| `common/src/snapshot/streaming_snapshot.rs` | 269 |
| `common/src/stake_addresses.rs` | 175 |
| `assets_state/src/state.rs` | 152 |
| `utxo_state/src/state.rs` | 136 |
| `drep_state/src/state.rs` | 134 |
| `spo_state/src/state.rs` | 116 |
| `consensus/src/consensus.rs` | 93 |

The nesting in `chain_store.rs` and `spo_state.rs` is pervasive (not isolated spots) and is structurally caused by the oversized `init`/`run` functions identified under ACR-009.

---

## Section 5 — Safety and Correctness

### ACR-015 (`shall`) — `unsafe` isolation
**Status: ✅ COMPLIANT**

No `unsafe` blocks found outside of comments or test code across all modules and common crates.

### ACR-016 (`shall`) — No unjustified panic paths in production code
**Status: ❌ VIOLATED — widespread, highest severity**

#### `unwrap()` without justification in production paths

| Location | Pattern |
|---|---|
| `spo_state/src/state.rs:193,467,476,512,699,712,733` | `Mutex::lock().unwrap()` — 7 occurrences in production state handlers |
| `consensus/src/consensus_tree.rs:517` | `self.blocks.get_mut(&hash).unwrap()` — production logic path |

#### `expect()` without inline justification

| Location | Message |
|---|---|
| `midnight_state/src/state.rs:83,112,117,118,121,124,131` | `"UTxO index out of sync..."` — invariant, but no safety comment |
| `assets_state/src/state.rs:926,955,984,1009,1044,1045,1071,1072,1098,1127,1238,1274,1275,1301,1302,1330,1331` | `"info should be Some"` / `"record should exist"` |
| `historical_accounts_state/src/state.rs:108,227,238,396,412,424` | `"window should never be empty"` |
| `address_state/src/state.rs:178` | `"window should never be empty"` |
| `common/src/snapshot/protocol_parameters.rs:371,375,378,382,383` | `"Current params must have..."` |
| `peer_network_interface/src/peer_network_interface.rs:56,58` | `"could not fetch genesis values"` |
| `utxo_state/src/state.rs:232` | `"total UTxO count went negative"` |
| `chain_store/src/stores/fjall.rs:179,281` | `"infallible"` — potentially valid but requires safety comment |
| `rest_blockfrost/src/handlers/epochs.rs:234` | `"epoch_number must exist for EpochParameters"` |
| `rest_blockfrost/src/utils.rs:146,160` | `"failed to convert"` / `"should be able to decode"` |
| `tx_unpacker/src/tx_unpacker.rs:139` | `"invalid tx hash length"` |
| `tx_unpacker/src/validations/phase2.rs:90` | `"Failed to create evaluator thread pool"` |
| `accounts_state/src/spo_distribution_store.rs:247,280,292,308,318` | `"Failed to create/store SPDD..."` |
| `common/src/address.rs:775,780,785,790,795,798` | Bech32/hash conversion expects in test helpers |

#### `panic!` in production message-handling paths

| Location | Message |
|---|---|
| `block_kes_validator/src/block_kes_validator.rs:118` | Unexpected message in genesis completion |
| `block_vrf_validator/src/block_vrf_validator.rs:133` | Unexpected message in genesis completion |
| `mithril_snapshot_fetcher/src/mithril_snapshot_fetcher.rs:460` | Unexpected bootstrapped message |
| `fake_block_injector/src/fake_block_injector.rs:157,170` | Unexpected bootstrapped/completion message |
| `tx_unpacker/src/tx_unpacker.rs:73` | Unexpected message type |
| `rest_blockfrost/src/handlers/blocks.rs:556,576` | `panic!` in HTTP handler match arms |
| `utxo_state/src/state.rs:827` | `"UTXO not found"` |
| `chain_store/src/chain_store.rs:142` | `"Corrupted DB"` — should be a recoverable `Result::Err` |
| `address_state/src/address_state.rs:143,168` | Panic on persistence worker crash |
| `historical_accounts_state/src/historical_accounts_state.rs:209` | Panic on persistence worker crash |
| `assets_state/src/address_state.rs:265,292,305` | Bare `panic!()` — no message or justification |
| `assets_state/src/assets_state.rs:213` | Panic in state handler |
| `stake_delta_filter/src/utils.rs:561` | `"Not a stake address"` |
| `parameters_state/src/parameters_state.rs:243` | `"ParametersState bootstrap failed"` |

> **Note:** `mithril_snapshot_fetcher/src/pause.rs:111,121`, `governance_state/src/conway_voting.rs:694`, `common/src/snapshot/decode.rs:337`, and `common/src/types.rs:2442` contain `panic!` in what appear to be test assertion helpers embedded in production files. These should be moved into `#[cfg(test)]` blocks if not already guarded.

#### `unreachable!` in crypto primitives

| Location | Note |
|---|---|
| `block_kes_validator/src/ouroboros/kes.rs:26,37,71` | Three `unreachable!` in KES signature construction |
| `block_vrf_validator/src/ouroboros/vrf.rs:185` | Unexpected VRF variant |
| `common/src/snapshot/protocol_parameters.rs:294` | Unexpected language version |

### ACR-019 (`shall`) — Explicit timeouts/retries at network/IO boundaries
**Status: ⚠️ GAP**

No explicit timeout, retry, or backoff patterns found in `peer_network_interface`, `genesis_bootstrapper`, `mithril_snapshot_fetcher`, or `snapshot_bootstrapper`. Network calls appear to rely on underlying library defaults without explicit timeout configuration.

---

## Section 6 — Concurrency and Async

### ACR-021 (`shall`) — No blocking ops on async executors
**Status: ✅ COMPLIANT WITH NOTES**

`spawn_blocking` is correctly used in `accounts_state`, `address_state`, and `mithril_snapshot_fetcher`. Rayon thread pools are properly used in `tx_unpacker/phase2.rs` and `spo_state/epochs_history.rs`. `parameters_state/build.rs` uses `reqwest::blocking` — acceptable as a build script, not runtime.

### ACR-022 (`will`) — Spawned task lifecycle documented
**Status: ⚠️ PARTIALLY DOCUMENTED**

The large `run()` / `init()` functions spawn subscription tasks via Caryatid's message bus, but task ownership and shutdown semantics are not explicitly documented at the call sites.

### ACR-023 (`shall`) — No lock held across await points
**Status: ⚠️ SPOT RISK**

`accounts_state/src/state.rs` wraps a `JoinHandle` in `Arc<Mutex<...>>` and calls `spawn_blocking`. The pattern itself is correct. However, the seven bare `Mutex::lock().unwrap()` calls in `spo_state/state.rs` lack any documented synchronization strategy and will panic on mutex poisoning — violating both ACR-016 and ACR-023's spirit.

### ACR-024 (`should`) — Bounded channels preferred
**Status: ✅ COMPLIANT**

No unbounded channels found in production paths.

---

## Section 7 — Dependencies and Libraries

### ACR-025 (`shall`) — Workspace dependency management
**Status: ❌ VIOLATED**

Dependencies declared locally (not via `workspace = true`) that should be in `[workspace.dependencies]`:

| Module | Local declaration |
|---|---|
| `mithril_snapshot_fetcher` | `mithril-client = "0.12"`, `mithril-common = "0.6.17"` |
| `snapshot_bootstrapper` | `reqwest = "0.12"`, `async-compression = "0.4.32"` |
| `rest_blockfrost` | `reqwest = "0.12"` |
| `genesis_bootstrapper` | `reqwest = "0.12"` |
| `parameters_state` | `reqwest = "0.11"` ⚠️ different major version |
| `governance_state` | `tracing-subscriber = "0.3.20"` |
| `address_state` | `tracing-subscriber = "0.3"` |
| `mcp_server` | `rmcp = "0.8"` (has inline comment justification, but still belongs in workspace) |

### ACR-027 (`will`) — No duplicate crates for the same concern
**Status: ❌ VIOLATED**

- **`reqwest`** is declared in 4 modules across two major versions: `"0.11"` (`parameters_state`) and `"0.12"` (3 other modules). Two versions of the same HTTP client are compiled into the workspace.
- **`tracing-subscriber`** appears as a local dependency in `address_state` and `governance_state`. Subscriber initialization belongs only in process crates, not library/module crates.

### ACR-028 (`shall`) — Critical-path crates pinned to major versions
**Status: ⚠️ CONCERN**

`mithril-client = { version = "0.12" }` and `mithril-common = { version = "0.6.17" }` are not pinned to exact patch versions. Given their role in snapshot verification (a security-relevant boundary), they should specify exact versions.

---

## Section 8 — Macros, Features, and Build Surface

### ACR-030–033
**Status: ✅ COMPLIANT**

The `#[module(...)]` procedural macro is from the Caryatid framework (a necessary external dependency). No feature-flag or `cfg` conditional issues observed.

---

## Section 9 — Error Handling, Logging, and Observability

### ACR-034 (`shall`) — Error types preserve actionable context
**Status: ❌ VIOLATED**

- `assets_state/src/address_state.rs:265,292,305`: bare `panic!()` with no message or context whatsoever
- Many `expect()` calls encode an invariant name but discard the original error, leaving no operational context on failure

### ACR-035 (`will`) — Structured log fields in critical flows
**Status: ⚠️ PARTIAL**

Inconsistent across modules. Examples of missing structure:

| Location | Log call | Missing fields |
|---|---|---|
| `consensus/src/consensus.rs:177` | `error!("Block message read failed")` | hash, slot, topic |
| `consensus/src/consensus.rs:201` | `error!("Consensus message read failed")` | hash, slot, topic |

Good examples exist (e.g. `warn!("Block {} rejected by tree: {e}", block_info.number)`) but coverage is not uniform across error paths.

### ACR-036 (`shall`) — Traceable identifiers in consensus/state-transition events
**Status: ⚠️ PARTIAL**

Block hash and number are included in some consensus log messages but absent from error and rollback paths. State-transition events (rollback, reject) lack cross-module correlation identifiers.

### ACR-037 (`should`) — Metrics for throughput/backlog/latency
**Status: ❌ ABSENT**

No metrics instrumentation found (no `metrics`, `prometheus`, or equivalent crate usage) on network or pipeline boundaries.

---

## Section 10 — Testing

### ACR-040 (`will`) — Unit and integration test coverage
**Status: ⚠️ GAPS**

Modules with **zero test files**:

| Module | Test files |
|---|---|
| `spdd_state` | 0 |
| `midnight_state` | 0 |
| `mcp_server` | 0 |
| `historical_accounts_state` | 0 |
| `genesis_bootstrapper` | 0 |
| `fake_block_injector` | 0 |
| `drdd_state` | 0 |

Modules with good test coverage (for reference):

| Module | Test files |
|---|---|
| `tx_unpacker` | 7 |
| `snapshot_bootstrapper` | 6 |
| `utxo_state` | 5 |
| `accounts_state` | 4 |
| `governance_state` | 4 |
| `rest_blockfrost` | 4 |
| `spo_state` | 4 |

### ACR-041 (`should`) — Property-based tests for parsers/codecs
**Status: ❌ ABSENT**

No `proptest` or `quickcheck` usage found. Parsers, CBOR codecs, and state-machine transitions in `common/` have no property-based coverage.

### ACR-042 (`shall`) — Fork/reorg/replay edge-case tests for consensus
**Status: ✅ COMPLIANT**

`consensus/src/consensus_tree.rs` contains comprehensive fork/reorg tests:
- `test_fork_depth`
- `test_fork_topologies`
- `test_bounded_maxvalid_rejects_deep_fork`
- `test_chain_switch_fires_rollback`
- `test_multi_level_rollback`
- `test_rollback_fires_proposed_for_fetched_blocks_on_new_chain`
- `test_prune_preserves_fork_after_boundary`
- `test_check_block_wanted_not_wanted_for_unfavoured_fork`

### ACR-043 (`will`) — Coverage reporting for safety/consensus-critical crates
**Status: ❓ UNKNOWN**

No CI coverage configuration observed in the repository. Cannot confirm changed-line coverage is reported for safety- or consensus-critical crates.

---

## Section 11 — Documentation

### ACR-044 (`will`) — README per crate
**Status: ❌ VIOLATED — 15 of 30 modules missing**

| Status | Modules |
|---|---|
| ✅ Has README | `accounts_state`, `block_unpacker`, `consensus`, `epochs_state`, `fake_block_injector`, `genesis_bootstrapper`, `governance_state`, `mcp_server`, `mithril_snapshot_fetcher`, `peer_network_interface`, `snapshot_bootstrapper`, `stake_delta_filter`, `tx_submitter`, `tx_unpacker`, `utxo_state` |
| ❌ Missing README | `address_state`, `assets_state`, `block_kes_validator`, `block_vrf_validator`, `chain_store`, `custom_indexer`, `drdd_state`, `drep_state`, `historical_accounts_state`, `historical_epochs_state`, `midnight_state`, `parameters_state`, `rest_blockfrost`, `spdd_state`, `spo_state` |

### ACR-045 (`shall`) — Rustdoc on public APIs and non-obvious invariants
**Status: ❌ VIOLATED**

`common/src/messages.rs` exposes 30+ public message structs (`RawBlockMessage`, `UTXODeltasMessage`, `SPOStateMessage`, `GovernanceProceduresMessage`, etc.) with no rustdoc comments. The `expect()` calls encoding structural invariants (`"window should never be empty"`, `"UTxO index out of sync"`) are not accompanied by a justifying safety comment at the call site.

---

## Prioritised Action List

### 🔴 Critical — `shall` violations requiring immediate action or deviation docs

| # | Rule | Action |
|---|---|---|
| 1 | **ACR-011** | Split the 14 files exceeding 800 LSLOC. Start with `streaming_snapshot.rs`, `accounts_state/state.rs`, `common/types.rs` |
| 2 | **ACR-016** | Audit all `panic!` / `unwrap` / bare `expect` in production paths; convert to `Result` propagation or add mandatory inline justification comments |
| 3 | **ACR-014** | Refactor deep nesting in `chain_store.rs`, `accounts_state/accounts_state.rs`, `spo_state/spo_state.rs` — primarily by extracting from oversized `run()`/`init()` functions |

### 🟠 High — `will` violations

| # | Rule | Action |
|---|---|---|
| 4 | **ACR-009** | Extract `streaming_snapshot::parse` (~707 lines), `accounts_state::run/init` (~444/399 lines), `spo_state::run/init` (~374/435 lines), `chain_store::handle_blocks_query` (~322 lines) |
| 5 | **ACR-025/027** | Move `reqwest`, `mithril-*`, `rmcp`, `async-compression`, `tracing-subscriber` into `[workspace.dependencies]`; eliminate `reqwest 0.11` or upgrade to match the workspace |
| 6 | **ACR-044** | Add READMEs to the 15 modules missing them |

### 🟡 Recommended — `should` violations and gaps

| # | Rule | Action |
|---|---|---|
| 7 | **ACR-019** | Add explicit timeouts to `peer_network_interface`, `genesis_bootstrapper`, `mithril_snapshot_fetcher` |
| 8 | **ACR-028** | Pin `mithril-client` and `mithril-common` to exact patch versions |
| 9 | **ACR-041** | Introduce `proptest` for CBOR codec round-trips and state-machine transitions in `common/` |
| 10 | **ACR-037** | Add basic metrics (block/tx throughput, queue depth) at network and pipeline boundaries |
| 11 | **ACR-045** | Add rustdoc to all public types in `common/src/messages.rs` and document `expect()` invariants with safety comments |
| 12 | **ACR-040** | Add at least minimal tests to `midnight_state`, `historical_accounts_state`, `genesis_bootstrapper`, `spdd_state` |
| 13 | **ACR-043** | Configure `cargo llvm-cov` or equivalent in CI with coverage reporting for `consensus` and `common` |

---

*This report should be reviewed quarterly alongside the rules document. Deviations for `shall` rules must be documented in the relevant module file or README with rule ID, justification, issue link, and revisit date.*
