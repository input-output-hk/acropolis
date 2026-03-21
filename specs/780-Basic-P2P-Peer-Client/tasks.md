# Tasks: Basic P2P Peer Discovery for PNI

**Feature Branch**: `780-Basic-P2P-Peer-Client`
**Input**: Design documents from `/specs/780-Basic-P2P-Peer-Client/`
**Methodology**: TDD — every task follows Red → Green → Refactor per `quickstart.md`
**Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel with other [P] tasks at same phase (different files, no shared dependency)
- **[US#]**: User story this task belongs to
- TDD Red tasks must complete before their Green counterparts begin

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Create file stubs and module wiring so that all test files compile (with failing tests — the Red state).

**Checkpoint**: `cargo build -p acropolis_module_peer_network_interface` succeeds with new empty stubs.

- [X] T001 Create empty stub `modules/peer_network_interface/src/peer_manager.rs` with placeholder `pub struct PeerManager;`
- [X] T002 [P] Create empty stub `modules/peer_network_interface/src/peer_sharing.rs` with placeholder `pub async fn request_peers() {}`
- [X] T002a Define `PeerSharingError` enum in `modules/peer_network_interface/src/peer_sharing.rs` using `#[derive(thiserror::Error, Debug)]` with variants: `CborDecode(String)`, `HandshakeFailed(String)`, `ConnectionFailed(String)`, `Timeout`; update `request_peers` return type to `Result<Vec<String>, PeerSharingError>` (constitution §1 — thiserror for new error types)
- [X] T003 Add `pub mod peer_manager;` and `pub mod peer_sharing;` to `modules/peer_network_interface/src/lib.rs`
- [X] T004 [P] Create empty test file `modules/peer_network_interface/tests/peer_manager_tests.rs`
- [X] T004b [P] Create empty test file `modules/peer_network_interface/tests/configuration_tests.rs` (required so T006 Red task has a pre-existing file to write into and Phase 1 checkpoint compiles)
- [X] T004c [P] Verify `rand` crate is available to `acropolis_module_peer_network_interface`: check `modules/peer_network_interface/Cargo.toml`; if absent, add `rand = { workspace = true }` (required by T014 random take and T031 random eviction)
- [X] T005 [P] Create empty test file `modules/peer_network_interface/tests/peer_sharing_tests.rs`

---

## Phase 2: Foundation — Configuration (Blocking Prerequisite)

**Purpose**: Add the 5 new config fields. All user stories depend on config values existing.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

**Checkpoint**: `cargo test -p acropolis_module_peer_network_interface` compiles and all pre-existing tests still pass.

- [X] T006 [TDD Red] Write `default_config_has_expected_peer_counts` test in `modules/peer_network_interface/tests/configuration_tests.rs` asserting `target_peer_count=15`, `min_hot_peers=3`, `peer_sharing_enabled=true`, `churn_interval_secs=600`, `peer_sharing_timeout_secs=10`; run `cargo test` → compilation error confirms Red
- [X] T007 Add 5 fields to `InterfaceConfig` in `modules/peer_network_interface/src/configuration.rs`: `target_peer_count: usize`, `min_hot_peers: usize`, `peer_sharing_enabled: bool`, `churn_interval_secs: u64`, `peer_sharing_timeout_secs: u64` — each with `#[serde(default)]` and matching `Default` impl values
- [X] T008 [P] Add 5 keys to `modules/peer_network_interface/config.default.toml`: `target-peer-count = 15`, `min-hot-peers = 3`, `peer-sharing-enabled = true`, `churn-interval-secs = 600`, `peer-sharing-timeout-secs = 10`
- [X] T009 Update `modules/peer_network_interface/src/peer_network_interface.rs` to read all 5 new config fields from `InterfaceConfig` and add them to the `NetworkManager::new()` call site (extend the call signature only — **do not** implement PeerManager construction here; that is T017); update `NetworkManager::new()` signature stub in `network.rs` to accept the new params as `_` placeholders so it compiles; run `cargo test` → T006 test passes

---

## Phase 3: US1 — Node Survives Peer Loss (Priority: P1) 🎯 MVP

**Goal**: When connected hot peers disconnect, the node promotes cold peers seeded from `node-addresses` to restore `min-hot-peers` connectivity without operator action.

**Independent Test**: Configure with 3 peers (`min-hot-peers=2`), disconnect 2 — node promotes cold peers and resumes syncing within 60 seconds (SC-001).

### TDD Red — Write failing tests first

- [X] T010 [US1] Write failing unit test stubs in `modules/peer_network_interface/tests/peer_manager_tests.rs`: `cold_list_seeded_from_config_excludes_hot`, `cold_list_deduplicates_on_seed`, `take_cold_peer_returns_none_when_empty`, `failed_peer_not_re_added_by_discovery`, `hot_overshoot_on_reconnect`; run `cargo test` → compile errors confirm Red
- [X] T011 [US1] Write failing test stub in `modules/peer_network_interface/src/network.rs` (`#[cfg(test)]` block): `promotes_cold_peer_when_hot_drops_below_min` — verifies that `PeerEvent::Disconnected` triggers cold peer promotion; compile error confirms Red

### TDD Green — PeerManager core

- [X] T012 [US1] Implement `PeerManagerConfig` struct and `PeerManager::new(config: PeerManagerConfig) -> Self` in `modules/peer_network_interface/src/peer_manager.rs`
- [X] T013 [US1] Implement `PeerManager::seed(&mut self, addresses: &[String], hot: &HashSet<String>)`: insert each address into `cold_peers` if not already in `hot` set; deduplicate via `HashSet` semantics
- [X] T014 [US1] Implement `PeerManager::take_cold_peer(&mut self) -> Option<String>`: randomly removes and returns one address from `cold_peers` (use `rand`); returns `None` when empty
- [X] T015 [US1] Implement `PeerManager::mark_failed(&mut self, address: String)`: remove from `cold_peers` (if present), insert into `failed_peers` session blacklist; run `cargo test peer_manager` → T010 tests pass

### TDD Green — NetworkManager promotion wiring

- [X] T016 [US1] Add `peer_manager: Option<PeerManager>` field to `NetworkManager` struct in `modules/peer_network_interface/src/network.rs`
- [X] T017 [US1] Implement the bodies of the new params in `NetworkManager::new()` (replacing the `_` stubs from T009): construct `PeerManager` from config (when `peer_sharing_enabled`), call `seed()` with `node-addresses`, then promote only up to `min_hot_peers` initial connections instead of all addresses (FR-002)
- [X] T018 [US1] Implement private `try_promote_cold_peer(&mut self)` in `network.rs`: call `peer_manager.take_cold_peer()`, spawn `PeerConnection`, **insert into `self.peers` at spawn time** (before TCP connect — D-012 invariant), call `peer_manager.mark_failed()` on connection failure
- [X] T019 [US1] In `NetworkManager::on_network_event()`, handle `PeerEvent::Disconnected`: remove from `self.peers`, then if `self.peers.len() < min_hot_peers` call `try_promote_cold_peer()`; run `cargo test` → T011 test passes
- [X] T020 [US1] Add FR-012 log lines for US1 events in `network.rs`: `info!` peer promoted cold→hot (with address, hot/cold counts), `warn!` cold peer promotion failed (with address, error), `info!` peer counts after each promotion

### TDD Refactor

- [X] T021 [US1] Refactor `peer_manager.rs`: extract private `is_known(&self, addr: &str, hot: &HashSet<String>) -> bool` helper checking cold + hot + failed sets; verify all T010–T011 tests still pass

---

## Phase 4: US2 — Node Discovers New Peers Organically (Priority: P2)

**Goal**: The node queries connected hot peers via the peer-sharing mini-protocol and accumulates validated `"ip:port"` addresses in its cold set, refreshing it continuously.

**Independent Test**: Node started with 3 configured peers accumulates ≥10 known addresses within 10 minutes on preprod (SC-002).

### TDD Red — Write failing tests first

- [X] T022 [P] [US2] Write failing integration tests in `modules/peer_network_interface/tests/peer_sharing_tests.rs` using a mock `tokio::net::TcpListener`: `happy_path_returns_decoded_ipv4_addresses`, `timeout_is_respected_when_server_hangs`, `v10_peer_returns_empty_without_error`, `malformed_cbor_response_returns_error`, `response_truncated_at_amount` (mock server sends 1000 entries, verify only `amount` returned); compile errors confirm Red
- [X] T023 [P] [US2] Write failing address validation unit tests in `modules/peer_network_interface/tests/peer_sharing_tests.rs`: `loopback_ipv4_rejected`, `loopback_ipv6_rejected`, `private_10_rejected`, `private_172_rejected`, `private_192_rejected`, `link_local_ipv4_rejected`, `port_zero_rejected`, `ipv4_mapped_ipv6_normalised_to_ipv4`, `public_ipv4_accepted`; compile errors confirm Red
- [X] T024 [US2] Write failing unit test stubs in `modules/peer_network_interface/tests/peer_manager_tests.rs`: `cold_list_cap_enforced_by_eviction`, `eviction_keeps_size_at_cap`, `peer_sharing_runs_at_cap`, `discovery_runs_above_target_count`, `cooldown_blocks_requery_within_five_minutes`, `cooldown_allows_requery_after_five_minutes`, `cold_list_deduplicates_on_discovery`; compile errors confirm Red
- [X] T025 [US2] Write failing test stub `peers_discovered_event_adds_to_cold_list` in `modules/peer_network_interface/src/network.rs` (`#[cfg(test)]` block) — verifies `NetworkEvent::PeersDiscovered` routes addresses into `PeerManager`; compile error confirms Red

### TDD Green — peer_sharing.rs address validation

- [X] T026 [US2] Implement `validate_and_normalise(addr: PeerAddress) -> Option<String>` in `modules/peer_network_interface/src/peer_sharing.rs`: reject loopback (127.x, ::1), unspecified (0.0.0.0, ::), link-local (169.254.x, fe80::/10), private RFC1918 (10.x, 172.16–31.x, 192.168.x), port 0; normalise `::ffff:x.x.x.x` → IPv4 form; run `cargo test` → T023 validation tests pass

### TDD Green — peer_sharing.rs protocol implementation

- [X] T027 [US2] Implement `decode_response(bytes: &[u8], amount: u8) -> Result<Vec<PeerAddress>, PeerSharingError>` in `peer_sharing.rs`: decode CBOR `MsgSharePeers` list, stop after `amount` entries and call minicbor `skip()` on remaining bytes without allocating (FR-014); return `Err` on malformed CBOR
- [X] T028 [US2] Implement TCP connect + plexer + V11 handshake + CBOR exchange in `request_peers_inner()` in `peer_sharing.rs`: `Bearer::connect_tcp(address)`, `Plexer::new(bearer)`, subscribe channel 0 (handshake) and channel 10 (`PROTOCOL_N2N_PEER_SHARING: u16 = 10`), `plexer.spawn()`, handshake with `v11_and_above(magic as u64)`, send `[0, amount]` via `AgentChannel::enqueue_chunk()`, receive `MsgSharePeers`, send `[2]` (MsgDone), `plexer.abort()`; run `cargo test` → T022 happy path test passes
- [X] T029 [US2] Wrap `request_peers_inner()` with `tokio::time::timeout(Duration::from_secs(timeout_secs), ...)` in public `request_peers()`: on timeout return `Ok(vec![])` and log warn; run `cargo test` → T022 timeout test passes
- [X] T030 [US2] Handle V11 handshake rejection in `request_peers()`: catch `Err` from handshake negotiation when peer only supports V7–V10, log at `debug!`, return `Ok(vec![])`; run `cargo test` → T022 v10 and malformed tests pass

### TDD Green — PeerManager discovery methods

- [X] T031 [US2] Implement `PeerManager::add_discovered(&mut self, addresses: Vec<String>, hot: &HashSet<String>)` in `peer_manager.rs`: for each address, skip if in cold/hot/failed sets; if `cold_peers.len() >= cold_cap` evict one random existing entry before inserting (D-010); run `cargo test` → T024 cap/eviction/dedup tests pass
- [X] T032 [US2] Implement `PeerManager::needs_discovery(&self, hot_count: usize) -> bool` returning `self.config.peer_sharing_enabled && hot_count > 0` (D-015 — continuous, not count-gated); implement `can_query(peer_id: PeerId) -> bool` checking 5-minute cooldown; implement `record_query(&mut self, peer_id: PeerId)` storing `Instant::now()`; run `cargo test` → T024 cooldown + discovery trigger tests pass
- [X] T033 [US2] Implement `PeerManager::cold_count(&self) -> usize` returning `self.cold_peers.len()`

### TDD Green — NetworkManager discovery wiring

- [X] T034 [US2] Add `PeersDiscovered { from_peer: PeerId, addresses: Vec<String> }` variant to `NetworkEvent` enum in `modules/peer_network_interface/src/network.rs`
- [X] T035 [US2] Restructure `NetworkManager::run()` to `tokio::select!` over `events.recv()`, `churn_ticker.tick()` (guarded by `peer_manager.is_some()`), and `discovery_ticker.tick()` (guarded by `peer_manager.is_some()`); create both tickers in `NetworkManager::new()` using `tokio::time::interval` (D-005); the discovery interval is a **hardcoded 60-second constant** (`const DISCOVERY_INTERVAL: Duration = Duration::from_secs(60)`) — it is not configurable (not in FR-009); the churn interval uses `config.churn_interval_secs`
- [X] T036 [US2] Implement private `on_discovery(&mut self)` in `network.rs`: if `needs_discovery(hot_count)`, collect cooldown-eligible hot peers via `can_query()`, randomly select one, call `record_query(peer_id)` immediately (before async spawn — D-006 timing invariant), spawn `request_peers(addr, magic, target_peer_count as u8, timeout)` task, send `PeersDiscovered` event on completion (empty addresses on failure)
- [X] T037 [US2] Handle `NetworkEvent::PeersDiscovered` in event loop: call `peer_manager.add_discovered(addresses, &hot_addr_set)`; run `cargo test` → T025 test passes
- [X] T038 [US2] Add FR-012 log lines for US2 events: `info!` peer-sharing complete (peer, discovered count, cold count), `warn!` peer-sharing exchange failed (peer, error), `debug!` V11 not supported (peer), `info!` cold peer evicted at cap (evicted address, cold count)

### TDD Refactor

- [X] T039 [US2] Refactor `peer_sharing.rs`: ensure `encode_request()` and `decode_response()` are clean private helpers with clear contracts; verify all T022–T023 tests still pass with no behaviour change

---

## Phase 5: US3 — Peer Churn (Priority: P3)

**Goal**: On a configurable interval, one randomly selected hot peer (above `min-hot-peers`) is demoted to cold and replaced, preventing permanent lock-in to a fixed peer set.

**Independent Test**: Over a 1-hour run with `churn-interval-secs=60` and cold peers available, at least one hot peer replacement per interval is logged (SC-003).

### TDD Red — Write failing tests first

- [X] T040 [US3] Write failing unit test stubs in `modules/peer_network_interface/tests/peer_manager_tests.rs`: `churn_allowed_when_above_min`, `churn_blocked_at_min_hot_peers`; compile errors confirm Red
- [X] T041 [US3] Write failing test stubs in `modules/peer_network_interface/src/network.rs` (`#[cfg(test)]` block): `churn_demotes_random_hot_peer_above_min`, `churn_does_not_demote_at_min_hot_peers`; compile errors confirm Red

### TDD Green

- [X] T042 [US3] Implement `PeerManager::should_churn(&self, hot_count: usize) -> bool` in `peer_manager.rs`: return `hot_count > self.config.min_hot_peers`; run `cargo test peer_manager` → T040 tests pass
- [X] T043 [US3] **blockedBy T035** — Implement private `on_churn(&mut self)` in `modules/peer_network_interface/src/network.rs`: if `should_churn(hot_count)`, randomly select one hot peer from `self.peers`, disconnect it (remove from `peers`), add its address to `cold_peers` via `peer_manager.add_discovered()`, call `try_promote_cold_peer()` to restore count; the `churn_ticker` arm in `tokio::select!` is the stub created by T035 — this task adds the function body only
- [X] T044 [US3] **blockedBy T035** — Wire `churn_ticker.tick()` arm (stub from T035) to call `on_churn()`; run `cargo test` → T041 tests pass
- [X] T045 [US3] Add FR-012 log lines for US3 events: `info!` peer demoted hot→cold with churn reason (address, hot/cold counts), `info!` peer counts after demotion

### TDD Refactor

- [X] T046 [US3] Refactor `network.rs`: verify `on_churn()`, `on_discovery()`, and `try_promote_cold_peer()` have no duplicated logic; run full `cargo test -p acropolis_module_peer_network_interface` — all tests pass

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Disabled mode, full FR-012 audit, clippy, fmt, final validation.

- [X] T047 Write `disabled_mode_no_op` test: construct `PeerManager` with `peer_sharing_enabled=false`, assert `needs_discovery(3)` returns false; verify `NetworkManager` with disabled config produces no discovery/churn ticks (FR-010)
- [X] T048 FR-012 logging audit: trace through `network.rs` and `peer_sharing.rs` confirming all 8 required log events are present at correct levels — see plan.md Logging section for exact field names and messages
- [X] T048a Add doc comments to all public items in `modules/peer_network_interface/src/peer_manager.rs` and `modules/peer_network_interface/src/peer_sharing.rs` (constitution §4 MUST — "All public types/functions must have doc comments"): `PeerManager`, `PeerManagerConfig`, all `pub fn` methods, `request_peers`; add `#[deny(missing_docs)]` crate-level attribute to both files to make future violations a compile error
- [X] T048b Add extension-point comments at all 8 locations in new code (see plan.md "Extension Points" section for exact comment text): (1) `connection.rs` PeerConnection spawn site — warm-peers phase-split; (2) `network.rs` `NetworkManager` struct — `warm_peers` field placeholder; (3) `network.rs` `try_promote_cold_peer()` — cold→warm two-phase note; (4) `network.rs` `on_churn()` — hot→warm demotion note; (5) `peer_manager.rs` `PeerManagerConfig` — `max_warm_peers` field note; (6) `peer_manager.rs` `PeerManager` struct — `ledger_peers` tracked set note; (7) `peer_manager.rs` `add_discovered()` — `seed_from_ledger()` sibling note; (8) `network.rs` `tokio::select!` loop — ledger peers `SPOStateMessage` subscription note
- [X] T049 [P] Run `cargo clippy -p acropolis_module_peer_network_interface -- -D warnings` and resolve all warnings
- [X] T050 [P] Run `cargo fmt --check -p acropolis_module_peer_network_interface` and apply formatting
- [X] T051 Run `cargo test -p acropolis_module_peer_network_interface` — full suite passes including pre-existing `chain_state`, `block_flow`, and `connection` tests with zero regressions

---

## Dependencies

```
Phase 1 (T001–T005, T002a, T004b, T004c) → Phase 2 (T006–T009) → Phase 3 (T010–T021)
                                                                 → Phase 4 (T022–T039) [requires Phase 3 complete]
                                                                 → Phase 5 (T040–T046) [requires Phase 3 complete; can parallel with Phase 4]
Phase 3 + Phase 4 + Phase 5 → Phase 6 (T047–T051, T048a, T048b)
```

Within Phase 5: T043 and T044 **blockedBy T035** (Phase 4) — the `churn_ticker` arm stub in `tokio::select!` must exist before it can be wired.

Phase 4 and Phase 5 share `network.rs` modifications — coordinate on `tokio::select!` arms to avoid merge conflicts if worked in parallel.

## Parallel Execution Examples

**Within Phase 4** (after T021 complete):
- T022 and T023 can run simultaneously (both write to `peer_sharing_tests.rs` test stubs — but different test functions)
- T026 (validate_and_normalise) is independent of T027/T028/T029/T030 (protocol impl)

**Phase 4 and Phase 5 in parallel** (after Phase 3 complete):
- Phase 5 T040–T042 can run concurrently with Phase 4 (`should_churn()` and its tests have no Phase 4 dependency)
- T043 and T044 must wait for T035 (Phase 4) — the `churn_ticker` arm stub must exist first

**Phase 6** (after all stories):
- T049 and T050 are [P] — clippy and fmt can run concurrently

## Implementation Strategy

**MVP scope (Phase 1 + 2 + 3)**: After T021, the node survives peer loss by promoting from a config-seeded cold list. This satisfies SC-001 and is shippable.

**Full scope (+ Phase 4)**: Adds organic peer discovery. Satisfies SC-002 and SC-004.

**Complete scope (+ Phase 5 + 6)**: Adds churn and hardening. Satisfies SC-003 and SC-005.
