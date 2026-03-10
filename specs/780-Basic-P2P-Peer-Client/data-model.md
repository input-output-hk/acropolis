# Data Model: Basic P2P Peer Discovery for PNI

**Branch**: `780-Basic-P2P-Peer-Client` | **Date**: 2026-03-09

All state is in-memory. No persistence layer is involved.

---

## Entities

### PeerManager

Central in-memory store for cold peer state and peer-sharing rate limiting. Lives inside `NetworkManager`.

```
PeerManager {
    cold_peers:       HashSet<String>              // "host:port" addresses not currently connected
    failed_peers:     HashSet<String>              // session blacklist: failed cold promotion attempts
    sharing_cooldown: HashMap<PeerId, Instant>     // last peer-sharing query time per hot peer
    config:           PeerManagerConfig            // target counts, caps, timeouts
}
```

**Invariants**:
- `cold_peers.len() <= config.target_peer_count * 4` — enforced by random eviction on every at-cap insertion
- No address in `cold_peers` appears in the current hot peer set (`NetworkManager.peers`)
- No address in `cold_peers` appears in `failed_peers`
- `sharing_cooldown` entries are only present for peers that have been queried at least once
- `failed_peers` is cleared only on process restart (session-scoped blacklist)

---

### PeerManagerConfig

Snapshot of discovery-related config fields passed into `PeerManager` at construction.

```
PeerManagerConfig {
    target_peer_count:          usize    // default 15
    min_hot_peers:              usize    // default 3
    peer_sharing_enabled:       bool     // default true
    churn_interval_secs:        u64      // default 600
    peer_sharing_timeout_secs:  u64      // default 10
}
```

**Derived constant**: `cold_cap = target_peer_count * 4`

---

### ColdPeer (logical, not a struct)

Represented as a `String` in `"host:port"` format within `cold_peers`. No additional metadata is stored per cold peer.

**Identity**: Exact string match. Two entries with different DNS names resolving to the same IP are treated as distinct.

**Lifecycle**:
```
[config node-addresses] ──► cold_peers  (startup seed)
[peer-sharing response] ──► cold_peers  (runtime discovery; runs continuously — cap enforced by eviction)
cold_peers ──► hot connection attempt   (promotion, via take_cold_peer())
  ├─ success → removed from cold_peers, added to NetworkManager.peers (peers.len() updated at spawn time, D-012)
  └─ failure → removed from cold_peers, mark_failed() → added to failed_peers (session blacklist, D-009)
hot peer (churn demotion) ──► cold_peers
[peer-sharing response addr] ──► cold_peers if NOT in failed_peers AND NOT in hot set
    ├─ cold_count < cap  → insert directly
    └─ cold_count >= cap → evict random cold peer, then insert (random eviction policy)
```

---

### PeerSharingExchange (transient)

A short-lived TCP connection opened to perform one peer-sharing round-trip. Not stored — created, used, and dropped within `peer_sharing::request_peers()`.

```
PeerSharingExchange {
    address:   String          // target hot peer address
    magic:     u32             // network magic for handshake
    timeout:   Duration        // from peer_sharing_timeout_secs config
}
→ Result<Vec<String>>          // discovered "ip:port" strings, or error
```

---

## State Transitions

### Hot Peer Count Transitions

```
hot_count < min_hot_peers
    └─► promote: take_cold_peer() → PeerConnection::new() spawned → peers.insert() (D-012: at spawn, not connect)
            ├─ connect success: hot_count++ (peers.len() already incremented at spawn)
            └─ connect failure: peers.remove() → mark_failed() → blacklisted (D-009)
                               → next PeerEvent::Disconnected triggers another promotion check

hot_count > min_hot_peers AND churn_ticker fires
    └─► demote: pick random hot peer → disconnect → add address to cold_peers
            └─ hot_count-- then trigger promotion check

hot_peer disconnects (PeerEvent::Disconnected)
    └─► existing reconnect logic (5s backoff) OR promote from cold if count < min_hot_peers
```

### Cold Peer Count Transitions

```
discovery_ticker fires AND needs_discovery() is true
    └─► filter hot peers to cooldown-eligible set
            ├─ empty set: skip tick
            └─ non-empty: randomly select one peer
                    └─► record_query(peer_id)  ← called BEFORE async exchange starts
                            └─► PeerSharingExchange
                                    ├─ success: add new addresses to cold_peers (dedup, eviction enforced)
                                    └─ failure: log at warn, skip (cooldown already recorded)
```

Note: `needs_discovery(hot_count)` returns true when `peer_sharing_enabled && hot_count > 0`. It is NOT gated on the total known peer count — discovery runs continuously to keep the cold set fresh (D-015). The cold cap is enforced at insertion via random eviction (D-010), not by suppressing discovery.

---

## New NetworkEvent Variants

```rust
// Added to existing NetworkEvent enum:
PeersDiscovered {
    from_peer: PeerId,
    addresses: Vec<String>,   // "ip:port" strings from peer-sharing response
}
```

---

## New Configuration Fields

Added to `InterfaceConfig` in `configuration.rs`:

| Field | Type | Default | Description |
|---|---|---|---|
| `target_peer_count` | `usize` | `15` | Defines cold peer cap (`cold_cap = 4 × target_peer_count`); no longer used as a discovery trigger |
| `min_hot_peers` | `usize` | `3` | Minimum active connections to maintain |
| `peer_sharing_enabled` | `bool` | `true` | Enable/disable all discovery behaviour |
| `churn_interval_secs` | `u64` | `600` | Seconds between random hot peer replacements |
| `peer_sharing_timeout_secs` | `u64` | `10` | Timeout for full peer-sharing exchange |
