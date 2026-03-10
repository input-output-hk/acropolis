# Research: Basic P2P Peer Discovery for PNI

**Branch**: `780-Basic-P2P-Peer-Client` | **Date**: 2026-03-09

## Decision Log

---

### D-001: Peer-Sharing Handshake Version

**Question**: Which N2N handshake version is required to use the peer-sharing mini-protocol?

**Decision**: Handshake version **V11 or above**.

**Rationale**: Pallas 0.34 defines `PEER_SHARING_DISABLED: u8 = 0` only for V11+. V7–V10 encode `VersionData` as a 2-element CBOR array with no peer-sharing field. V11–V14 encode as 4-element arrays including `peer_sharing: Option<u8>`. The function `handshake::n2n::VersionTable::v11_and_above(magic)` exists in pallas 0.34 and produces a table covering V11–V14.

**Implication**: The peer-sharing exchange must open a new TCP connection using `v11_and_above()` for handshake negotiation. If the remote peer only supports V7–V10 (old node), the handshake will fail and the exchange should be skipped gracefully.

**Alternatives considered**: Using `v7_and_above()` — rejected because it would negotiate V7–V10 on older peers, which have no peer-sharing capability, leading to protocol errors on channel 10.

---

### D-002: Peer-Sharing Connection Strategy

**Question**: Can peer-sharing be multiplexed onto the existing hot peer connection?

**Decision**: **No — a separate short-lived TCP connection is required.**

**Rationale**: `Plexer::subscribe_client()` must be called before `spawn()`. Once `RunningPlexer` is returned, the underlying `Demuxer`/`Muxer` are consumed into spawned tasks and cannot accept new channel registrations. The existing `PeerClient` (created by `PeerClient::connect()`) already has its plexer spawned. There is no way to add channel 10 to it post-hoc without modifying pallas.

**Implementation**: For each peer-sharing exchange:
1. `Bearer::connect_tcp(address)` → new TCP connection
2. `Plexer::new(bearer)` → subscribe channel 0 (handshake) + channel 10 (peer-sharing)
3. `plexer.spawn()` → start mux/demux tasks
4. Perform N2N handshake with `v11_and_above(magic)`
5. Send `[0, amount]` (MsgShareRequest), receive `[1, [...]]` (MsgSharePeers), send `[2]` (MsgDone)
6. `plexer.abort()` → close connection

**Alternatives considered**: Patching pallas to support post-spawn subscription — rejected per FR-011.

---

### D-003: Peer-Sharing CBOR Encoding

**Question**: How are peer-sharing messages encoded without a pallas miniprotocol implementation?

**Decision**: **Manual CBOR encoding/decoding using `pallas_codec::minicbor` (v0.25.1)**.

**Message format** (from Ouroboros network spec v14 CDDL):
```
MsgShareRequest(amount: u8) = [0, amount]
MsgSharePeers(peers)        = [1, [[tag, ...addr_fields, port], ...]]
  IPv4: [0, u32_be, u16]
  IPv6: [1, u32, u32, u32, u32, u16]
MsgDone                     = [2]
```

**Implementation**: Define a local `PeerSharingMsg` enum with `minicbor::Encode`/`Decode` derives, or hand-write encode/decode using the minicbor encoder/decoder API directly. The protocol is simple enough (3 message types) to do inline without a full state machine.

**Note on channel framing**: Messages must be sent through `AgentChannel::enqueue_chunk()` which handles muxer framing. The CBOR bytes are the payload.

---

### D-004: Cold Peer Set Representation

**Question**: How should cold peer addresses be stored and deduplicated?

**Decision**: `HashSet<String>` keyed on the raw `"host:port"` string.

**Rationale**: Simple, zero allocation overhead for lookup, trivially serialisable for logging. DNS names are not resolved — two entries with different DNS names but the same IP are treated as different peers (acceptable for basic P2P). Addresses from peer-sharing arrive as `(Ipv4Addr, u16)` or `(Ipv6Addr, u16)` tuples which are formatted as `"ip:port"` strings on insertion.

**Alternatives considered**: Storing as `SocketAddr` — rejected because configured `node-addresses` may be DNS hostnames, making a uniform `SocketAddr` type impractical without DNS resolution.

---

### D-005: Timer Integration in NetworkManager

**Question**: How are periodic timers (churn, peer-sharing check) integrated into the event loop?

**Decision**: **`tokio::select!` over the existing `events.recv()` and new `tokio::time::interval` timers.**

**Rationale**: The current `NetworkManager::run()` loop is `while let Some(event) = self.events.recv().await`. Adding `tokio::select!` with interval futures is idiomatic Tokio and requires minimal restructuring. Two intervals: `churn_ticker` (every `churn_interval_secs`) and `discovery_ticker` (every 60 seconds, checks if discovery is needed).

**Alternative considered**: Sending synthetic timer events via the `events` channel — rejected as it pollutes the event type with infrastructure concerns.

---

### D-006: Peer-Sharing Cooldown Tracking and Peer Selection Algorithm

**Question**: How is the 5-minute per-peer cooldown enforced? What is the full peer selection algorithm?

**Decision**: `HashMap<PeerId, Instant>` in `PeerManager`, checked against `Instant::now()` before each query selection. The full selection algorithm on each discovery tick is:

1. Collect all current hot peer IDs from `NetworkManager.peers`.
2. Filter to those where `can_query(peer_id)` returns true (i.e. not in `sharing_cooldown` or last queried more than 5 minutes ago).
3. If no eligible peers → skip this tick (log at trace level).
4. Randomly select one peer from the eligible set.
5. **Call `record_query(peer_id)` immediately** — before spawning the async exchange task.
6. Spawn the peer-sharing task. On completion (success or failure), send `PeersDiscovered` event (empty `addresses` on failure is fine).

**Critical timing invariant**: `record_query()` is called in step 5, before the async task, not in the completion handler. This prevents a second discovery tick (fired while a slow exchange is still in progress) from selecting the same peer again. Without this, two concurrent exchanges to the same peer could run simultaneously.

**Cooldown applies on both success and failure**: A failed exchange (timeout, connection error, CBOR decode error) still sets the cooldown. The peer was attempted; retrying immediately would just fail again. The 5-minute window is sufficient for transient failures to resolve.

**Rationale**: `tokio::time::Instant` is monotonic and cheaply compared. No additional async machinery needed — the cooldown check is synchronous and inline. Random selection among cooldown-eligible peers is sufficient for "basic P2P": it avoids hammering the same peer, avoids recently-failed peers, and requires no quality scoring infrastructure.

**Quality-based selection explicitly ruled out**: The spec Assumptions state "Peer selection for churn and discovery uses random selection, not quality-based scoring." RTT measurement and byte counting are out of scope. If quality-based selection is added in a future feature, the `sharing_cooldown` map can be replaced with a `PeerScore` struct without changing the selection interface.

---

### D-012: Concurrent Outbound Connection Limit

**Question**: What prevents unbounded concurrent TCP connection attempts when many cold peers are promoted simultaneously?

**Decision**: `min_hot_peers` is the implicit concurrent connection limit, enforced by counting *spawned-but-not-yet-connected* peers in `peers.len()` at promotion check time. No separate config knob is needed.

**The storm scenario**: If `hot_count` counted only established connections, then on simultaneous disconnection of multiple hot peers, the promotion loop could fire once per disconnect event before any replacement connects — promoting far more peers than `min_hot_peers` concurrently.

**The fix**: A peer is added to `NetworkManager.peers` when its `PeerConnection` is spawned (at promotion time), not when the underlying TCP connection is established. `PeerEvent::Disconnected` removes it from `peers`. The promotion check becomes:

```
if peers.len() < min_hot_peers {
    promote_one_cold_peer()   // adds to peers immediately
}
```

This makes `peers.len()` a count of (connected + connecting) peers, and caps concurrent outbound attempts to `min_hot_peers` at all times.

**Worst-case cascade with bad cold list**: if all cold peers fail to connect, the pattern is sequential batches of `min_hot_peers` attempts (each batch resolves before the next starts), not 60 simultaneous attempts. With `min_hot_peers = 3`, at most 3 concurrent TCP connects at any time.

**Alternatives considered**:
- New `max-outbound-connection-attempts` config — rejected; `min_hot_peers` already expresses the operator's intent about how many connections to maintain, and using it as the concurrency bound is the simplest correct solution.
- Explicit `pending_connections: usize` counter — rejected; it duplicates information already in `peers.len()` if peers are added at spawn time.
- Exponential backoff between promotion batches — deferred; acceptable for a future improvement if the sequential-batch behaviour proves too aggressive in practice.

**Implementation note**: The existing `PeerConnection::new()` in `connection.rs` immediately returns a `PeerConnection` struct (before TCP connect). The calling code in `NetworkManager` must add this to `peers` before returning from the promotion handler. This is the key invariant that enforces the limit.

---

### D-011: Peer Address Validation from Peer-Sharing Responses

**Question**: Should addresses returned by the peer-sharing protocol be validated before entering the cold peer set? What constitutes a valid address?

**Decision**: Yes — peer-sharing is an untrusted source and addresses must be validated in `peer_sharing.rs` before returning to the caller. Invalid addresses are silently dropped; an all-invalid response is treated as `Ok(vec![])`.

**Validation rules applied in order:**

| Rule | Rejected examples | Rationale |
|---|---|---|
| Reject loopback | `127.0.0.1`, `127.x.x.x`, `::1` | Prevents connection to self or local services |
| Reject unspecified | `0.0.0.0`, `::` | Malformed / meaningless address |
| Reject link-local | `169.254.x.x`, `fe80::/10` | Unreachable from public internet |
| Reject private RFC1918 | `10.x.x.x`, `172.16–31.x.x`, `192.168.x.x` | Unreachable for public network peers; SSRF-like risk on LAN |
| Reject port 0 | `1.2.3.4:0` | Invalid — OS-assigned ephemeral port, not a listening address |
| Normalize IPv4-mapped IPv6 | `::ffff:1.2.3.4` → `1.2.3.4` | Prevents treating the same peer as two different addresses |

**What is NOT filtered:**
- Ports 1–1023: non-standard but valid (Cardano doesn't mandate port 3001)
- Self-address (our own public IP): **not required** — see rationale below.

**Where validation occurs**: In `peer_sharing.rs`, applied to the decoded `Vec<PeerAddress>` before converting to `Vec<String>`. The `peer_sharing::request_peers()` return value only contains validated, normalised addresses.

**Self-address filtering — explicitly not required:**
When a peer returns our own public IP:port (which Cardano nodes commonly do), it enters the cold set and a connection attempt is made. Since this feature has no inbound server, the attempt fails immediately with `ECONNREFUSED`. The address is moved to `failed_peers` and never tried again. Cost: one TCP connection attempt per session. This is acceptable.

If an inbound server is added in a future feature, self-connection would succeed and cause a ChainSync loop — at that point self-address filtering becomes mandatory. For now it is deferred.

An optional `self-address` config hint (`String`) may be documented for operators who wish to skip the single failed attempt, but it is not part of the functional requirements.

**Alternatives considered**:
- Validate in `PeerManager::add_discovered()` — rejected; validation should happen at the trust boundary (point of receipt from untrusted peer), not inside internal state management.
- Allow private RFC1918 ranges — rejected for the mainnet/preprod use case; private IPs discovered via peer-sharing are unreachable and waste cold peer slots. Operators who need local peers configure them directly in `node-addresses`.
- Filter ports < 1024 — rejected as too aggressive; some nodes legitimately listen on low ports.
- Require self-address config for filtering — rejected; the failed_peers blacklist handles it naturally at negligible cost.

---

### D-010: Cold Peer Set Eviction Policy

**Question**: When the cold peer set is at capacity, how are existing entries removed to make room for new ones?

**Decision**: **Random eviction on every at-cap insertion.** When inserting a peer address (from peer-sharing or churn demotion) and `cold_count >= cold_cap`, one randomly selected existing cold peer is evicted before the new address is inserted. The cold set size is always maintained at or below `cold_cap = 4 × target_peer_count`.

**Corollary**: The "suppress peer-sharing when at cap" behavior from the initial design is **removed**. Suppression was a workaround for the absence of eviction. With random eviction, peer-sharing runs at its normal cadence and the cap is enforced by replacing stale entries, keeping the cold set fresh.

**Rationale**: Without eviction, the cold set becomes permanently stale once full — peer-sharing is suppressed and no new addresses ever enter. Random eviction is the simplest strategy that solves this: it requires no additional state (no timestamps, no scores), is consistent with the random-everywhere approach used for churn and peer selection, and ensures the cold set continuously turns over with newly discovered addresses.

**Alternatives considered**:
- LRU eviction (remove oldest entry) — would provide better freshness guarantees but requires an ordered structure (e.g. `IndexMap` or `VecDeque` + `HashSet`) instead of a plain `HashSet`. The complexity is not justified for basic P2P since random eviction already provides adequate freshness.
- Score-based eviction (remove lowest quality peer) — requires per-peer success/failure tracking. No such data is collected in this feature.
- Suppression only (no eviction) — rejected as it causes the staleness problem the user identified.
- Eviction only on churn demotion, suppression for peer-sharing — rejected as unnecessarily complex split policy; random eviction is uniformly applied.

**Impact on FR-004a**: The existing requirement text ("peer-sharing requests MUST be suppressed until the cold list shrinks below the cap") is replaced with the eviction policy.

---

### D-009: Repeated Connection Failure Handling

**Question**: What happens when a peer fails to connect repeatedly — either during cold promotion or as an existing hot peer?

**Decision**: Two separate policies, one per connection tier.

**Cold peer promotion failure policy — session blacklist:**
When a cold peer fails to connect during promotion, it is discarded from the cold set and added to a `failed_peers: HashSet<String>` session blacklist. If that address is later returned by peer-sharing, it is silently ignored on insertion. The blacklist is in-memory only (cleared on restart). No retry is attempted for failed cold peers during the same session.

Rationale: Cold peers are tried opportunistically. A failed cold peer wastes a promotion slot if retried. The blacklist prevents peer-sharing from repeatedly re-adding a known-bad address within the same run. On restart, the blacklist is cleared — the peer may have recovered.

**Hot peer reconnection policy — unchanged from existing:**
The existing `PeerConnection` worker reconnects indefinitely with a 5-second backoff delay. This mechanism is not modified by this feature. When a hot peer disconnects, `PeerEvent::Disconnected` fires and we promote a cold peer if `hot_count < min_hot_peers`. If the original hot peer reconnects, the node temporarily has `min_hot_peers + 1` hot connections. This is allowed — the excess will be resolved at the next churn tick.

No additional failure tracking (failure counts, exponential backoff, permanent bans) is added for hot peers. The 5-second reconnect backoff is sufficient for basic P2P.

**Alternatives considered**:
- Exponential backoff on cold peers — rejected; cold peers are not retried at all, making backoff irrelevant.
- Failure count + blacklist threshold for hot peers — rejected; the existing reconnect loop already self-limits via fixed 5s backoff, and persistent hot peer failure is a network-level issue outside this feature's scope.
- Re-adding failed cold peers after a timeout — rejected as unnecessary complexity for basic P2P.

**Impact on data model**: `PeerManager` gains `failed_peers: HashSet<String>`. The `add_discovered()` method must filter against this set on insertion.

---

### D-008: Warm Peer Tier — Needed or Not?

**Question**: Does the implementation require a warm peer tier (connected but not running ChainSync/BlockFetch)?

**Decision**: **No warm tier for this feature.** All active connections are hot (ChainSync + BlockFetch). The warm tier is deferred.

**Rationale**: In the full Ouroboros governor, warm peers serve two purposes: (1) run peer-sharing cheaply on an existing multiplexed connection, and (2) enable fast cold→hot promotion since the TCP connection is already open. Both are worked around in this design:

1. **Peer-sharing**: We open a dedicated short-lived TCP connection per exchange. The overhead is negligible given the 60-second discovery tick and 5-minute per-peer cooldown — at most ~1 extra TCP connection per minute.

2. **Fast promotion**: Cold→hot connects directly. The TCP + handshake latency is acceptable since promotion is infrequent (only triggered when hot count falls below `min_hot_peers`).

Adding a warm tier would require: a third connection state, separate lifecycle management (keepalive-only connections), warm→hot promotion logic, warm peer set tracking, and additional edge case handling across `PeerManager` and `NetworkManager`. This complexity is not justified given that all spec success criteria (SC-001–SC-005) are achievable without it.

**When to add it**: If future requirements include RTT-based peer scoring (requires keepalive on a maintained connection), serving inbound peer-sharing requests, or sub-second hot promotion latency — a warm tier becomes worthwhile.

**Alternatives considered**: Implementing warm tier now — rejected as over-engineering for "basic P2P". The spec explicitly scopes this out.

---

### D-007: Handshake Failure on V11 Negotiation

**Question**: What happens if a hot peer doesn't support V11+ (old Cardano node)?

**Decision**: Log at debug level and skip that peer for peer-sharing. Select a different hot peer on the next discovery tick. The peer remains hot for block sync (its existing V7+ connection is unaffected).

**Rationale**: The separate peer-sharing connection is independent of the existing hot connection. A failure on channel 10 negotiation does not affect ChainSync/BlockFetch on the original connection.

---

### D-013: Inbound Connection Handling

**Question**: Should the implementation accept inbound connections from other nodes? If so, do inbound peers count as hot, participate in peer-sharing, and get churned?

**Decision**: **Inbound connections are explicitly out of scope for this feature.** This node acts as a client only — it makes outbound connections; it does not listen for incoming ones. All three sub-questions (hot counting, peer-sharing participation, churn) are N/A.

**Rationale**:

1. **No listener infrastructure**: Acting as a server requires binding a port, which involves public IP discovery, NAT traversal / port forwarding knowledge, and configurable listen addresses. None of this exists in the current PNI module.

2. **Asymmetric churn semantics**: We can churn our own outbound connections. We cannot churn inbound connections — the remote peer decides when to disconnect. Mixing inbound peers into the hot set would require a separate "do not churn inbound" policy, adding complexity without benefit to this feature.

3. **Success criteria are achievable without inbound**: SC-001–SC-005 are purely about the node's ability to find and maintain enough outbound connections to sync blocks. Inbound connections contribute nothing to these criteria.

4. **Peer-sharing already discovers enough peers**: Cold peer discovery via peer-sharing from outbound connections is sufficient to build a large peer set. Inbound connections would be redundant as a discovery source.

**Impact**: The `hot_count` tracked by `NetworkManager` counts only outbound connections. `peers` contains only connections we initiated. This is the correct invariant for the outbound-only model.

**When inbound becomes necessary**: If a future feature requires the node to serve other nodes (e.g., acting as a relay or public bootstrap node), inbound handling must be added — including: `TcpListener`, inbound peer tracking (separate from outbound), inbound peer-sharing serving, and the decision on whether inbound peers count toward `min_hot_peers`.

**Alternatives considered**:
- Accept inbound now — rejected; requires port binding infrastructure and adds significant complexity outside this feature's scope.
- Count inbound peers in `hot_count` — rejected for the same reason; they don't exist yet.
- Partially implement inbound (accept connections but not count them) — rejected as confusing and unnecessary.

---

### D-015: Discovery Trigger Policy

**Question**: Should peer-sharing run only when the total known peer count falls below `target-peer-count`, or should it run continuously?

**Decision**: **Run peer-sharing on every discovery tick, unconditionally (subject only to `peer_sharing_enabled` and having at least one hot peer).** The count-based trigger condition `(cold_count + hot_count) < target_peer_count` is removed.

**Rationale — the staleness problem with count-based triggering**:

The count condition `total < target` halts discovery once the node has accumulated enough *addresses*. But addresses age — peers go offline, move IPs, get restarted on different ports. A full cold set of 60 entries that were valid two weeks ago may have 40% failures today. The count stays at 60, so discovery never runs, and the node silently carries a degraded cold set until a cascade of promotion failures forces a rebuild from scratch.

The 5-minute per-peer cooldown (D-006) is already the correct rate-limiting mechanism. With 3 hot peers and a 60-second tick, peer-sharing runs at most once per minute, cycling through hot peers at most once per 5 minutes each. This is a negligible load — one short-lived TCP connection per minute.

**New trigger condition**:
```
discovery_ticker fires
    AND peer_sharing_enabled == true
    AND hot_count > 0
    AND any hot peer has can_query() == true
```
`needs_discovery()` is simplified to: `self.config.peer_sharing_enabled && hot_count > 0`. The method returns false only if peer-sharing is disabled or there are no hot peers to query.

**Role of `target_peer_count` after this change**: The field is retained but its role changes — it is used solely to define the cold cap (`cold_cap = 4 × target_peer_count`). It no longer acts as a discovery throttle.

**Staleness is handled by D-010 (random eviction)**: Continuous peer-sharing means new addresses continuously trickle into the cold set, evicting stale ones via random eviction. Over time, live addresses crowd out dead ones organically.

**Alternatives considered**:
- Keep count-based trigger — rejected; causes the staleness problem described above. A node running for weeks against a busy network will accumulate a full but progressively stale cold list with no mechanism to refresh it.
- Time-based trigger (run if last discovery was > N minutes ago, regardless of count) — equivalent to removing the count condition but adds a `last_discovery: Instant` field. No benefit over always running since the cooldown already enforces minimum inter-query spacing.
- Count-based trigger with a separate staleness timer — adds both a field and complexity. The simpler solution (always run) has the same effect.

---

### D-016: Peer Freshness Tracking

**Question**: Should cold peers carry a last-seen timestamp? Without one, dead addresses accumulate in the cold set indefinitely.

**Decision**: **No last-seen timestamps for this feature.** Dead peer accumulation is adequately handled by the combination of three existing mechanisms without adding per-peer metadata.

**How freshness is maintained without timestamps:**

1. **Tried-and-failed peers are session-blacklisted (D-009)**: Any cold peer that fails promotion is immediately removed from `cold_peers` and added to `failed_peers`. It cannot re-enter the cold set for the remainder of the session. This handles dead peers that are actually promoted.

2. **Continuous peer-sharing replaces stale entries (D-015 + D-010)**: Discovery runs on every tick. At cap, each insertion evicts a random cold peer. Statistically, after N insertions into a cap-60 cold set, `1 - (59/60)^N` of the original entries have been replaced. After 120 insertions (~2 hours with 3 hot peers), ~86% of original entries are gone. The cold set turns over organically.

3. **Session restart clears state**: `failed_peers` resets on restart. Cold peers are re-seeded from config (a small, operator-curated list) and rebuilt via peer-sharing from fresh network responses. A restart is a clean slate.

**What timestamps would add (deferred):**
- **LRU eviction**: Replace random eviction with "evict oldest" — ensures fresh addresses survive longer. Requires replacing `HashSet<String>` with `IndexMap<String, Instant>` or `VecDeque` + `HashSet`. Complexity not justified for basic P2P since random eviction already provides adequate turnover.
- **TTL expiry**: Remove cold peers not re-discovered within N days. Requires a background scan or lazy expiry on access. Adds the same structural change as LRU. Useful if the cold set should shrink proactively rather than wait for eviction.

**Why random eviction is sufficient for basic P2P:** Dead peers that are never promoted (because hot count stays healthy) are evicted randomly over time as discovery continues. The node does not need to distinguish fresh from stale entries to meet SC-001–SC-005. The worst case is a slightly higher promotion failure rate if many cold entries are stale, which is handled gracefully by the session blacklist and next-candidate retry.

**Alternatives considered:**
- Add `HashMap<String, Instant>` for cold peers — rejected; structural change for marginal benefit in basic P2P. LRU eviction is a documented deferred improvement in D-010.
- TTL-based cold peer expiry — rejected; same structural cost, and continuous peer-sharing + random eviction achieves adequate freshness without a timer.
- Track failure count per cold peer — rejected; D-009 takes the simpler position of "one strike and you're out" for the session, which is correct for cold peers tried opportunistically.

---

### D-017: Address Canonicalization Scope

**Question**: Do addresses need canonicalization before deduplication? Addresses might appear as `1.2.3.4:3000`, `/ipv4/1.2.3.4/tcp/3000` (multiaddr), or DNS hostnames.

**Decision**: **No additional canonicalization is needed beyond what D-011 already specifies.** The address pipeline is narrow and produces a canonical `"ip:port"` string by construction. Multiaddr format is irrelevant to this implementation.

**Why the pipeline is already canonical:**

The Cardano peer-sharing protocol (Ouroboros, not libp2p) encodes peer addresses as binary tuples in CBOR:
- IPv4: `[0, u32_be, u16]` — 4-byte IP, 2-byte port
- IPv6: `[1, u32, u32, u32, u32, u16]` — 16-byte IP, 2-byte port

These decode directly to `PeerAddress::V4(Ipv4Addr, u16)` or `V6(Ipv6Addr, u16)`. There is no string parsing, no multiaddr format, no DNS name in peer-sharing responses — only raw binary IP addresses. `validate_and_normalise()` formats them as `format!("{}:{}", ip, port)`, which Rust's `Display` implementation renders deterministically:
- `Ipv4Addr` → `"1.2.3.4"` (always dotted-decimal, no leading zeros)
- `Ipv6Addr` → `"[::1]"` etc. (RFC 5952 compressed form)

The only ambiguity that exists — IPv4-mapped IPv6 `::ffff:1.2.3.4` — is explicitly normalised to `1.2.3.4` (D-011/FR-013).

**Config addresses**: Stored as-is from the operator's config. The operator controls the format. D-004 explicitly accepts that `relay.example.com:3000` and `1.2.3.4:3000` pointing to the same host are treated as distinct entries — DNS resolution is out of scope for basic P2P.

**Multiaddr format (`/ipv4/1.2.3.4/tcp/3000`)**: This is a libp2p concept. The Cardano/Ouroboros network stack does not use multiaddr. Pallas does not produce or accept multiaddr strings anywhere in the peer-sharing flow. This format is simply not present in the address pipeline.

**What would require canonicalization (deferred):** If future sources add DNS resolution, multiaddr parsing, or alternate string formats (e.g. topology files, ledger peers), a normalization step would be needed at the ingress point of each new source. That normalization would live at the source boundary (alongside address validation), not in `PeerManager`.

**Alternatives considered:**
- Parse and normalize all config addresses to `SocketAddr` on startup — rejected; DNS names cannot round-trip through `SocketAddr`, and D-004 already accepts DNS-vs-IP as distinct entries intentionally.
- Add multiaddr parsing — rejected; not needed for Ouroboros/Cardano peer-sharing, which uses binary encoding, not multiaddr strings.

---

### D-018: Peer Scoring

**Question**: Should peers carry a score (success count, latency, failure count) used for promotion priority, eviction preference, and discovery query selection?

**Decision**: **No peer scoring in this feature.** Peer selection for all three use cases (promotion, eviction, discovery queries) uses random selection. This is already stated in the spec Assumptions: "Peer selection for churn and discovery uses random selection, not quality-based scoring."

**Per-use-case analysis:**

**Promotion (cold → hot selection)**:
Cold peers have never been connected. There is no latency, success, or failure data to score them on before the first connection attempt. A score of zero for all cold peers is equivalent to random selection. The only available signal is the session blacklist (D-009): peers that previously failed are excluded entirely, which is already implemented.

**Eviction (cold peer removal at cap)**:
Same problem — cold peers are unconnected. No observable quality signal exists. Score-based eviction (prefer to evict lowest-quality peers) would require either: (a) keeping failed cold peers in the cold set with a failure flag instead of blacklisting them — contradicts D-009, or (b) measuring cold peer quality without connecting to them — impossible. Random eviction (D-010) is correct given the absence of observable data.

**Discovery queries (hot peer selection for peer-sharing)**:
This is where scoring would be most meaningful: prefer hot peers that have responded quickly with many valid addresses in the past. However, measuring hot peer quality requires pallas instrumentation — RTT on keepalive messages, bytes received per peer, etc. This is explicitly out of scope (spec Assumptions: "Round-trip time measurement and byte counting (connection instrumentation) are explicitly out of scope"; FR-011: no pallas modifications). The 5-minute cooldown (D-006) already prevents hammering any single peer, which is the only practically meaningful constraint without instrumentation.

**What scoring would require (deferred):**

If peer scoring is added in a future feature:
1. **Hot peer instrumentation**: Wrap `PeerClient` (pallas) to track: bytes received, last-block time, keepalive RTT. Requires either pallas modification or a shim layer around the existing `PeerConnection`.
2. **Score struct**: Replace `sharing_cooldown: HashMap<PeerId, Instant>` with `peer_scores: HashMap<PeerId, PeerScore>` where `PeerScore` carries cooldown + quality metrics. D-006 explicitly notes this upgrade path.
3. **Cold peer score inheritance**: On promotion (cold → hot), a new `PeerScore` is created with zero data. Scores accumulate only after connection.
4. **Weighted random selection**: Replace uniform random with weighted sampling by score for discovery and churn demotion. The `rand::distributions::WeightedIndex` in the `rand` crate supports this without major restructuring.

**Alternatives considered:**
- Add basic success/failure counter per cold peer — rejected; cold peers never accumulate this data before promotion, making the counter always zero for unconnected peers.
- Track peer-sharing response quality per hot peer (response size, valid addresses returned) — rejected for this feature; meaningful metric but requires storing per-peer history, adding state complexity not justified for basic P2P.
- Prefer hot peers that have been connected longest for peer-sharing queries — rejected; connection duration is a proxy for stability but not quality, and it would bias toward a small set of long-lived peers, reducing network sampling diversity.

---

### D-019: Global Rate Limiting for Peer-Sharing

**Question**: Is a global rate limit (e.g. max peer-sharing requests per minute) needed to prevent abuse?

**Decision**: **No additional global rate limit is needed.** The existing design already enforces a rate that is more conservative than any reasonable rate-limit config would set. No new config knob is added.

**Structural rate constraints already in place:**

This node is a *client* of the peer-sharing protocol only (D-013: no inbound server). The only traffic this feature generates is outbound peer-sharing exchanges we initiate. Three independent mechanisms cap this rate:

| Mechanism | Limit |
|---|---|
| Discovery tick interval (D-005) | 1 exchange per 60 seconds — structural ceiling, not configurable |
| Per-peer cooldown (D-006) | Each hot peer queried at most once per 5 minutes |
| Hot peer count (`min_hot_peers`) | With 3 hot peers: at most 3 exchanges per 15 minutes = 12/hour = 0.2/min |

The effective global rate is the minimum of these: **at most 1 peer-sharing TCP connection per 60-second tick**. A typical "max N requests per minute" config would be set to 1–5, which is *more permissive* than the 1/minute structural ceiling already enforced.

**No inbound exposure:**
"Peer-sharing requests could be abused" typically refers to a server being flooded with inbound requests. This node has no inbound peer-sharing server (D-013). Remote peers cannot make peer-sharing requests to us. There is no inbound surface to rate-limit.

**Alternatives considered:**
- Add `max-peer-sharing-per-minute` config — rejected; the discovery tick interval already provides a tighter and simpler global limit. A config knob would either duplicate the tick interval (redundant) or be set lower than 1/min (pointless given the tick is already 60 seconds). If an operator wants slower discovery, they increase `churn-interval-secs` or set a longer discovery tick — not an explicit rate limit.
- Token bucket / leaky bucket implementation — rejected; over-engineering for a 1/minute outbound exchange rate. Token buckets are appropriate for high-frequency request patterns (hundreds/second), not one request per minute.
- Rate limit when inbound server is added — correct future action; if D-013 is revisited and an inbound peer-sharing server is implemented, an inbound rate limit (per-source IP, global) MUST be added at that point.

---

### D-020: Network Partition Handling

**Question**: During a network partition, the node may lose many peers simultaneously and reconnect aggressively. Does this cause a connection storm? Are additional limits needed?

**Decision**: **No additional limits needed.** D-012 already bounds concurrent outbound connection attempts to `min_hot_peers` at all times, including during simultaneous disconnections. The existing hot peer reconnect loop (5-second backoff, indefinite retry) handles partition recovery.

**How the partition scenario plays out:**

*Phase 1 — partition hits, multiple disconnections*:

Events fire in rapid succession as hot peers disconnect. For each disconnect, the promotion check runs: `if peers.len() < min_hot_peers { promote_one_cold_peer() }`. Because `peers.len()` includes spawned-but-not-yet-connected peers (D-012 invariant), each promotion immediately restores `peers.len()` to `min_hot_peers` before the next disconnect event is processed. Result: exactly `min_hot_peers` (e.g. 3) concurrent TCP connection attempts, never more. Not a storm.

*Phase 2 — cold list exhaustion*:

If all promoted cold peers also fail (unreachable during partition), each failure removes the peer from `peers`, triggers another promotion, until the cold list is exhausted. The pattern is sequential batches of `min_hot_peers` TCP attempts (each batch fails and resolves before the next starts — D-012: "worst-case cascade" note). With `min_hot_peers = 3` and 60 cold peers: 20 sequential batches, 3 concurrent attempts each. At a conservative 10-second connect timeout per attempt: ~3–10 minutes to exhaust cold list. This is bounded and not aggressive.

*Phase 3 — total isolation (cold list empty, hot count = 0)*:

`needs_discovery()` returns false (no hot peers to query). Discovery goes silent. The configured hot peers (from `node-addresses`) continue their existing `PeerConnection` reconnect loop: 5-second backoff, indefinite retry. No new state, no new logic required. The node waits for the partition to resolve.

*Phase 4 — partition ends*:

One configured hot peer reconnects through the existing retry loop → `peers.len()` increases → `PeerEvent::Connected` fires → discovery resumes → cold list rebuilds via peer-sharing within minutes.

**Why no exponential backoff on cold peer promotion**:
D-012 explicitly considered and deferred exponential backoff between promotion batches: "acceptable for a future improvement if the sequential-batch behaviour proves too aggressive in practice." For basic P2P, the 3-concurrent-attempt bound is sufficient. A 10-second TCP connect timeout means the worst case is 3 wasted connections at a time — negligible.

**What the partition scenario does NOT cause**:
- Unbounded concurrent TCP connects: impossible due to D-012.
- Hot peer session blacklisting: hot peers use indefinite reconnect with 5-second backoff (D-009); they are never added to `failed_peers`.
- Discovery runaway: `needs_discovery()` returns false when `hot_count = 0`, silencing discovery during total isolation.

**Alternatives considered:**
- Exponential backoff between promotion batches — deferred per D-012; acceptable if sequential batches prove too aggressive in practice.
- Circuit breaker on cold peer promotion (pause after N failures) — rejected; the sequential-batch pattern already self-limits via the D-012 invariant without additional state.
- Jitter on promotion timing — rejected; promotion is triggered by disconnect events, not a periodic timer. Jitter is useful for periodic retry loops (hot peer reconnect already has 5s fixed backoff, which is sufficient).

---

### D-021: Peer-Sharing Response Size Limit

**Question**: Can a malicious peer return an unbounded number of addresses in `MsgSharePeers`, causing memory amplification?

**Decision**: **Yes, this is a real risk — and it is mitigated by truncating the decoded response at `amount` entries.** The `decode_response()` helper MUST stop decoding CBOR list entries after `amount` items and discard the remainder without allocation. This is FR-014.

**The amplification vector:**

The Ouroboros peer-sharing protocol sends `MsgShareRequest(amount: u8)` to indicate how many addresses we want. The responder SHOULD honour this, but a malicious (or buggy) peer can send any number of entries in `MsgSharePeers`. The CBOR list length is encoded in the message header but elements are decoded sequentially — without a truncation guard, the decoder would allocate a `Vec` entry for every element in the response before any validation occurs.

Attack budget:
- `MsgSharePeers` with 100,000 IPv4 entries: each `[0, u32, u16]` ≈ 8 bytes CBOR → ~800KB on the wire
- 10-second timeout limits data received, but at even 1 Mbps: ~1.25MB ≤ 156,250 entries possible
- Each decoded `PeerAddress::V4(Ipv4Addr, u16)` = 6 bytes → 100K entries = ~600KB transient allocation
- Subsequent validation (all rejected as private/loopback garbage) = 100K iterations of `validate_and_normalise()`

**The fix — truncate at `amount` during decode:**

In `decode_response()`, after decoding `amount` entries from the CBOR list, return immediately and discard the remaining bytes. The decoded `Vec<PeerAddress>` is then bounded to `amount × sizeof(PeerAddress)` ≈ `15 × 6` = 90 bytes (with default `amount = target_peer_count = 15`). Even at the u8 maximum of 255 entries: 255 × 6 = 1.5KB. Memory amplification is eliminated.

**Implementation note for minicbor**: The `minicbor` API decodes arrays element-by-element. In a hand-written decoder, after reading `amount` elements from the array, call `skip()` on the decoder to consume remaining bytes without allocation, then return the partial Vec. This prevents both allocation and iteration cost for excess entries.

**`amount` value specification (previously unspecified):**

The `amount` sent in `MsgShareRequest` is `target_peer_count as u8` (default: 15). Rationale: we want enough addresses to fill a meaningful portion of the cold set without requesting more than we'll use. `target_peer_count` is the natural bound. It is capped at 255 (u8 max) for operators who set an unusually high target.

**Secondary bound — cold cap:**

Even without truncation, the cold set cap (D-010: `4 × target_peer_count = 60`) would limit how many addresses actually enter the cold set. But this is a logical bound on *insertions*, not on *decoding/allocation* — the amplification occurs before this bound applies. Truncation addresses the allocation risk at the earliest possible point.

**Alternatives considered:**
- Rely on cold cap as the bound — rejected; the cold cap limits insertions into the cold set but does not prevent decoding a large Vec first. Memory amplification occurs before the cold cap is applied.
- Configurable `max-peers-per-response` — rejected; `amount` already serves this purpose. A separate config adds redundancy. If the operator wants fewer addresses per response, they reduce `target-peer-count`.
- Validate and truncate after full decode — rejected; full decode allocates the Vec first. Truncation must happen during decode to prevent the allocation.
