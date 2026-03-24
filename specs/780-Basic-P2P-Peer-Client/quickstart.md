# Quickstart: Implementing P2P Peer Discovery in PNI

**Branch**: `780-Basic-P2P-Peer-Client` | **Date**: 2026-03-09
**Methodology**: Test-Driven Development — every step follows Red → Green → Refactor.

This guide walks through the implementation order, key integration points, and TDD cycles for each piece.

---

## TDD Ground Rules

For every implementation step:

1. **Red**: Write the failing test(s) first. Run `cargo test` — they must fail.
2. **Green**: Write the minimum code to make the tests pass. No extra logic.
3. **Refactor**: Clean up without changing behaviour. Tests must still pass.

Never write production code before a failing test exists for it.

---

## Prerequisites

- Familiar with the existing PNI module (`modules/peer_network_interface/`)
- `cargo test -p acropolis_module_peer_network_interface` passes on `main`
- pallas 0.34 in workspace (confirmed — no upgrade needed)

---

## Implementation Order

Work in this order — each step produces independently passing tests before the next begins.

---

### Step 1: Configuration

**Files**: `configuration.rs`, `config.default.toml`

#### Red
```rust
// tests/configuration_tests.rs
#[test]
fn default_config_has_expected_peer_counts() {
    let config = InterfaceConfig::default();
    assert_eq!(config.target_peer_count, 15);
    assert_eq!(config.min_hot_peers, 3);
    assert!(config.peer_sharing_enabled);
    assert_eq!(config.churn_interval_secs, 600);
    assert_eq!(config.peer_sharing_timeout_secs, 10);
}
```
Run: `cargo test` → **compilation error** (fields don't exist yet). ✓ Red.

#### Green
Add the five fields to `InterfaceConfig` with defaults. Config file changes in `config.default.toml`.

Run: `cargo test` → test passes. ✓ Green.

#### Refactor
Ensure deserialization uses `#[serde(default)]` where appropriate. No behaviour change.

---

### Step 2: PeerManager — Core Logic

**File**: `modules/peer_network_interface/src/peer_manager.rs` (new)

Write all tests before any implementation. Each test must fail at compile or runtime before the corresponding method exists.

#### Red — write all tests first

```rust
// tests/peer_manager_tests.rs

#[test] fn cold_list_seeded_from_config_excludes_hot() { /* FR-002 */ }
#[test] fn cold_list_deduplicates_on_seed() { /* FR-005 */ }
#[test] fn cold_list_deduplicates_on_discovery() { /* FR-005 */ }
#[test] fn cold_list_cap_enforced_by_eviction() { /* FR-004a: insert at cap evicts one */ }
#[test] fn eviction_keeps_size_at_cap() { /* FR-004a: size never exceeds cap */ }
#[test] fn peer_sharing_runs_at_cap() { /* FR-004a: needs_discovery true even at cap */ }
#[test] fn discovery_runs_above_target_count() { /* D-015: needs_discovery true when cold+hot >= target */ }
#[test] fn failed_peer_not_re_added_by_discovery() { /* D-009 */ }
#[test] fn cooldown_blocks_requery_within_five_minutes() { /* FR-004b */ }
#[test] fn cooldown_allows_requery_after_five_minutes() { /* FR-004b */ }
#[test] fn churn_allowed_when_above_min() { /* FR-006 */ }
#[test] fn churn_blocked_at_min_hot_peers() { /* FR-007 */ }
#[test] fn take_cold_peer_returns_none_when_empty() { /* FR-003 edge */ }
```

Run: `cargo test` → compilation failures. ✓ Red.

#### Green — implement PeerManager methods one test at a time

For each test, implement the minimum code to make *that test* pass, then move to the next:

1. `new()` + `seed()` → passes `cold_list_seeded_from_config_excludes_hot`
2. Dedup logic in `seed()` → passes deduplication tests
3. `add_discovered()` with cap + eviction → passes cap/eviction tests
4. `needs_discovery()` without suppression at cap → passes `peer_sharing_runs_at_cap`
5. `mark_failed()` + filter in `add_discovered()` → passes failed peer test
6. `record_query()` + `can_query()` with `Instant` comparison → passes cooldown tests
7. `should_churn()` → passes churn tests
8. `take_cold_peer()` → passes take tests

After each method: run `cargo test peer_manager` — only the targeted test(s) should newly pass.

#### Refactor
Extract any repeated logic (e.g. `is_known(addr)` helper checking cold + hot + failed). Tests still pass.

---

### Step 3: Peer-Sharing Client

**File**: `modules/peer_network_interface/src/peer_sharing.rs` (new)

These are integration tests requiring a real TCP listener. Use `tokio::net::TcpListener` in test setup to act as a mock peer.

#### Red — write integration tests first

```rust
// tests/peer_sharing_tests.rs

#[tokio::test]
async fn happy_path_returns_decoded_ipv4_addresses() {
    // Spawn mock TCP server that: accepts V11 handshake, responds with MsgSharePeers
    // Call request_peers() pointing at mock server
    // Assert returned addresses match expected "ip:port" strings
}

#[tokio::test]
async fn timeout_is_respected_when_server_hangs() {
    // Spawn mock server that accepts connection but never responds
    // Call request_peers() with 1-second timeout
    // Assert completes within ~1.5s and returns Ok(vec![])
}

#[tokio::test]
async fn v10_peer_returns_empty_without_error() {
    // Spawn mock server that rejects V11+ and only accepts V7-V10
    // Call request_peers()
    // Assert Ok(vec![])
}

#[tokio::test]
async fn malformed_cbor_response_returns_error() {
    // Spawn mock server that sends garbage bytes after handshake
    // Assert Err(...)
}

#[tokio::test]
async fn response_truncated_at_amount() {
    // Spawn mock server that responds with 1000 valid IPv4 entries
    // Call request_peers() with amount=10
    // Assert returned Vec has exactly 10 entries (not 1000)
    // Assert no excessive allocation occurs (structural: only `amount` entries decoded)
}

// Address validation — unit tests, no TCP needed
#[test] fn loopback_ipv4_rejected() { assert!(validate_peer_address("127.0.0.1", 3001).is_err()); }
#[test] fn loopback_ipv6_rejected() { assert!(validate_peer_address("::1", 3001).is_err()); }
#[test] fn private_10_rejected() { assert!(validate_peer_address("10.0.0.1", 3001).is_err()); }
#[test] fn private_172_rejected() { assert!(validate_peer_address("172.16.0.1", 3001).is_err()); }
#[test] fn private_192_rejected() { assert!(validate_peer_address("192.168.1.1", 3001).is_err()); }
#[test] fn link_local_ipv4_rejected() { assert!(validate_peer_address("169.254.1.1", 3001).is_err()); }
#[test] fn port_zero_rejected() { assert!(validate_peer_address("1.2.3.4", 0).is_err()); }
#[test] fn ipv4_mapped_ipv6_normalised_to_ipv4() {
    let result = normalise_peer_address(Ipv6Addr::from([0,0,0,0,0,0xffff,0x0102,0x0304]), 3001);
    assert_eq!(result, Some("1.2.3.4:3001".to_string()));
}
#[test] fn public_ipv4_accepted() { assert!(validate_peer_address("185.40.4.100", 3001).is_ok()); }
```

Run: `cargo test peer_sharing` → compilation errors (module doesn't exist). ✓ Red.

#### Green — implement `request_peers()` against failing tests

Implement the TCP connect → plexer → V11 handshake → CBOR exchange → abort flow step by step, making one test pass at a time:

1. Stub returning `Ok(vec![])` → happy path fails (no addresses), others may pass
2. Add handshake + CBOR send/receive → happy path passes
3. Add timeout wrapping → timeout test passes
4. Add V11 check → V10 test passes
5. Add CBOR error handling → malformed test passes

#### Refactor
Extract CBOR encode/decode into private `encode_request()` / `decode_response()` helpers. Tests still pass.

---

### Step 4: Wire PeerManager into NetworkManager

**File**: `modules/peer_network_interface/src/network.rs`

#### Red — write tests against the new NetworkManager behaviour

```rust
// tests/network_manager_tests.rs (extend existing tests)

#[tokio::test]
async fn promotes_cold_peer_when_hot_drops_below_min() {
    // Build NetworkManager with 1 hot peer (min=2), 1 cold peer
    // Send PeerEvent::Disconnected
    // Assert cold peer was promoted (connection attempted)
}

#[tokio::test]
async fn churn_demotes_random_hot_peer_above_min() {
    // Build NetworkManager with 4 hot peers, min=2
    // Advance churn ticker
    // Assert hot_count == 3 after tick
}

#[tokio::test]
async fn churn_does_not_demote_at_min_hot_peers() {
    // Build NetworkManager with exactly min hot peers
    // Advance churn ticker
    // Assert hot_count unchanged
}

#[tokio::test]
async fn peers_discovered_event_adds_to_cold_list() {
    // Send PeersDiscovered with 5 addresses
    // Assert peer_manager.cold_count() == 5
}

#[tokio::test]
async fn disabled_mode_skips_all_discovery() {
    // Build with peer_sharing_enabled=false
    // Advance tickers
    // Assert no peer-sharing attempted, cold list empty
}
```

Run: `cargo test network_manager` → failures. ✓ Red.

#### Green
1. Add `peer_manager: Option<PeerManager>` to `NetworkManager`
2. Change `new()` to seed cold peers and connect only `min_hot_peers` initially
3. Add `PeersDiscovered` to `NetworkEvent`
4. Restructure `run()` to `tokio::select!` with churn + discovery tickers
5. Handle `PeerEvent::Disconnected` → promote cold peer if needed
6. Handle `PeersDiscovered` → `peer_manager.add_discovered()`
7. Add FR-012 log lines

Make one test pass at a time, re-running `cargo test` after each.

#### Refactor
Extract `on_churn()`, `on_discovery_tick()`, `try_promote_cold_peer()` into private methods. Verify all tests still pass including the pre-existing chain_state tests.

---

### Step 5: End-to-End Smoke Test

Run against preprod with config overrides:
```toml
target-peer-count = 10
min-hot-peers = 2
churn-interval-secs = 120
peer-sharing-timeout-secs = 10
```

Verify in logs (not automated):
- Cold peer seeding on startup
- Peer-sharing exchanges within first few minutes
- Churn demotions at ~2-minute intervals
- Hot/cold counts logged after each event

---

## Key Integration Points

### pallas types used by peer_sharing.rs

```rust
use pallas::network::{
    multiplexer::{Bearer, Plexer},
    miniprotocols::{handshake, PROTOCOL_N2N_HANDSHAKE},
};
use pallas_codec::minicbor;
```

Protocol ID 10 for peer-sharing has no constant in pallas 0.34 — define locally:
```rust
const PROTOCOL_N2N_PEER_SHARING: u16 = 10;
```

### Handshake for peer-sharing connection

```rust
// Use v11_and_above — not v7_and_above — to ensure peer-sharing capability
let versions = handshake::n2n::VersionTable::v11_and_above(magic as u64);
let confirmation = hs_client.handshake(versions).await?;
```

### Timeout wrapping

```rust
tokio::time::timeout(
    Duration::from_secs(config.peer_sharing_timeout_secs),
    request_peers_inner(address, magic, amount),
).await.unwrap_or_else(|_| { warn!("peer-sharing timed out"); Ok(vec![]) })
```

---

## What Does NOT Change

- `connection.rs` — no modifications needed
- `block_flow.rs` — no modifications needed
- `chain_state.rs` — no modifications needed
- `peer_network_interface.rs` — minor: pass new config fields to `NetworkManager::new()`
- Message bus topics — no new messages published or subscribed
