# Feature Specification: Basic P2P Peer Discovery for PNI

**Feature Branch**: `780-Basic-P2P-Peer-Client`
**Created**: 2026-03-05
**Status**: Draft
**Input**: Add basic P2P peer discovery and cold/warm/hot peer management to the PNI module

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Node Survives Peer Loss Without Manual Intervention (Priority: P1)

A node operator runs Acropolis syncing from the Cardano network. One or more of the statically configured backbone peers goes offline or becomes unreachable. The node must continue syncing without operator intervention by promoting peers from its cold list.

**Why this priority**: The most critical gap in the current PNI is that it only connects to statically configured peers. If all configured peers fail, syncing stops entirely. Resilience through a dynamic peer pool is the foundation everything else builds on.

**Independent Test**: Configure the node with 3 backbone peers. Simulate a network partition that takes 2 peers offline. Observe that the node discovers and connects to alternative peers and continues publishing blocks within a reasonable time.

**Acceptance Scenarios**:

1. **Given** a node connected to 3 configured peers, **When** 2 peers disconnect simultaneously, **Then** the node promotes cold peers and resumes syncing within 60 seconds without operator action.
2. **Given** a node with an empty cold peer list, **When** all configured peers disconnect, **Then** the node retries configured peers with backoff rather than stopping permanently.
3. **Given** a node with `min-hot-peers = 3`, **When** the hot peer count drops to 2, **Then** the node immediately attempts to promote a cold peer to restore the minimum.

---

### User Story 2 - Node Discovers New Peers Organically (Priority: P2)

A node operator starts a fresh node with a small set of bootstrap addresses. Over time, the node should expand its knowledge of the network by asking its connected peers for their known addresses, building a cold peer list for future use.

**Why this priority**: Without peer discovery, the node is permanently limited to its configured list. Organic discovery allows the node to participate in the broader P2P network and survive bootstrap address deprecation.

**Independent Test**: Start a node with 3 configured peers and `target-peer-count = 15`. After sufficient runtime, verify the node has discovered and recorded more than 3 peer addresses in its cold pool.

**Acceptance Scenarios**:

1. **Given** a node connected to hot peers, **When** the discovery tick fires and a cooldown-eligible hot peer exists, **Then** the node queries that peer for additional addresses using the peer-sharing protocol (discovery runs continuously, not only when below target).
2. **Given** a peer-sharing response containing valid IPv4/IPv6 addresses, **When** those addresses are received, **Then** they are added to the cold peer list (deduplicating against already-known addresses).
3. **Given** a peer-sharing response containing addresses already known or already connected, **When** those addresses are received, **Then** they are silently ignored without error.
4. **Given** a peer-sharing request fails or times out, **When** the failure occurs, **Then** the node logs the failure and continues operating without crashing.

---

### User Story 3 - Peer Churn Prevents Permanent Capture by a Peer Set (Priority: P3)

A node should not remain permanently connected to the same set of hot peers indefinitely. Periodic random replacement (churn) ensures the node samples different parts of the network over time and avoids inadvertent long-term dependency on a specific peer set.

**Why this priority**: Without churn, a node bootstrapped from a small set of peers stays locked to that set forever. Churn is necessary for decentralization even if it has no effect on correctness in the short term.

**Independent Test**: Run a node for an extended period with churn enabled. Observe that the set of hot peers changes over time — at least one peer is replaced per churn interval.

**Acceptance Scenarios**:

1. **Given** a node with more hot peers than `min-hot-peers`, **When** a churn interval elapses, **Then** one randomly selected hot peer is disconnected and moved to the cold list.
2. **Given** a hot peer demoted by churn, **When** it is moved to cold, **Then** the node attempts to promote another cold peer to maintain the hot peer count.
3. **Given** the hot peer count is exactly `min-hot-peers`, **When** a churn interval elapses, **Then** no demotion occurs (do not drop below minimum).

---

### Edge Cases

- What happens when the cold peer list is exhausted and no new peers are reachable? The node must not crash — it retries existing configured peers with backoff.
- What happens when a peer-sharing response contains malformed or unparseable addresses? Malformed entries are skipped; parseable ones are still used.
- What happens when a peer-sharing response contains loopback, private, or link-local addresses? They are silently rejected during validation — they never enter the cold peer set.
- What happens when a peer-sharing response returns an IPv4-mapped IPv6 address (e.g. `::ffff:1.2.3.4`)? It is normalised to its IPv4 form (`1.2.3.4`) before deduplication, preventing the same host from appearing twice with different string representations.
- What happens when a peer-sharing response contains port 0? That address entry is rejected.
- What happens when a newly promoted cold peer immediately fails to connect? It is discarded from the cold list, added to a session blacklist, and the next candidate is tried. If peer-sharing later returns that same address, it is ignored.
- What happens when a hot peer reconnects after we already promoted a cold peer to replace it? The node temporarily has more than `min-hot-peers` hot connections. This is allowed — the churn ticker resolves the excess at the next interval.
- What happens if a hot peer disconnects and reconnects repeatedly? The existing `PeerConnection` reconnect logic (5-second backoff, indefinite retry) handles this unchanged. No new failure tracking is added for hot peers.
- What happens when the node receives duplicate addresses from multiple peer-sharing responses? Deduplication by address string ensures the cold list contains no duplicates.
- What happens if `target-peer-count` is set lower than `min-hot-peers`? The system uses `min-hot-peers` as the effective floor for hot connections. `target-peer-count` now only affects the cold cap (`4 × target-peer-count`); it no longer gates peer-sharing.
- What happens when the cold list reaches `4 × target-peer-count`? A randomly selected existing cold peer is evicted to make room for the new address. Peer-sharing continues running — the cap is maintained by eviction, not suppression.
- What happens when `peer-sharing-enabled = false` and all configured peers go offline permanently? The node retries all configured peers indefinitely with 5-second backoff (the existing `PeerConnection` reconnect logic). It will not crash, but it cannot discover new peers and will remain isolated until at least one configured peer comes back online. This is intentional — `peer-sharing-enabled = false` is the pre-feature baseline mode (FR-010, SC-005). Operators who disable peer-sharing accept the responsibility of maintaining a reachable set of `node-addresses`. No additional fallback mechanism (DNS bootstrap, ledger peers, etc.) is provided.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST maintain a cold peer set — a collection of known peer addresses that are not currently connected.
- **FR-001a**: Peer addresses MUST come from exactly two sources: (1) the statically configured `node-addresses` list, which seeds the cold list on startup; and (2) the peer-sharing mini-protocol, which discovers additional addresses from connected hot peers at runtime. No other discovery sources (ledger peers, DNS bootstrap, topology files) are in scope.
- **FR-002**: The system MUST seed the cold peer set from the statically configured `node-addresses` on startup, connecting only up to `min-hot-peers` initially rather than all of them.
- **FR-003**: The system MUST promote a cold peer to a hot connection whenever the count of active hot peers falls below `min-hot-peers`.
- **FR-004**: The system MUST query a randomly selected hot peer for additional peer addresses on every discovery tick, provided `peer-sharing-enabled = true` and at least one hot peer is cooldown-eligible. Discovery runs continuously — it is NOT gated on the total known peer count falling below `target-peer-count`. The 5-minute per-peer cooldown (FR-004b) is the sole rate-limiting mechanism.
- **FR-004b**: The system MUST enforce a hardcoded 5-minute cooldown per hot peer between peer-sharing exchanges. A peer that was queried within the last 5 minutes MUST NOT be selected for another peer-sharing request.
- **FR-004a**: The cold peer list MUST NOT exceed `4 × target-peer-count` entries. When an insertion would breach this cap, the system MUST evict one randomly selected existing cold peer before inserting the new address. This eviction policy applies to all insertions (peer-sharing discoveries and churn demotions). Peer-sharing requests are NOT suppressed when the cap is reached.
- **FR-005**: The system MUST add newly discovered peer addresses to the cold peer list, deduplicating against addresses already known or connected.
- **FR-006**: The system MUST demote one randomly selected hot peer to cold on a configurable churn interval, provided the current hot peer count exceeds `min-hot-peers`.
- **FR-007**: The system MUST NOT drop the hot peer count below `min-hot-peers` due to churn.
- **FR-008**: The system MUST handle peer-sharing request failures gracefully (timeout or connection error) without disrupting ongoing block synchronisation. The full peer-sharing exchange (TCP connect + request + response) MUST be cancelled if it does not complete within `peer-sharing-timeout-secs`.
- **FR-009**: The system MUST be configurable with: `target-peer-count` (default 15), `min-hot-peers` (default 3), `peer-sharing-enabled` (default true), `churn-interval-secs` (default 600), `peer-sharing-timeout-secs` (default 10).
- **FR-010**: When `peer-sharing-enabled = false`, the system MUST skip peer discovery entirely and behave identically to the pre-feature baseline, preserving backward compatibility.
- **FR-011**: The system MUST NOT require any modifications to the pallas-network library.
- **FR-012**: The system MUST emit structured log lines for the following events, at the specified levels:
  - `info`: each peer promotion (cold→hot), including address and resulting hot/cold counts
  - `info`: each peer demotion (hot→cold via churn), including address and resulting hot/cold counts
  - `info`: each peer-sharing discovery batch completion, including peer queried and count of addresses received
  - `info`: hot/cold peer counts at each churn or promotion event
  - `warn`: cold peer promotion failure (connection refused, timeout, or error), including address and error
  - `warn`: peer-sharing exchange failure (connection error, handshake failure, CBOR decode error, or timeout), including peer address and failure reason
  - `info`: cold peer eviction (random eviction at cap), including evicted address and current cold count
  - `debug`: V11 handshake failure on a hot peer selected for peer-sharing (peer does not support V11+), including peer address
- **FR-013**: The system MUST validate all peer addresses received via peer-sharing before adding them to the cold peer set. The following MUST be rejected: loopback addresses (127.0.0.0/8, ::1), unspecified addresses (0.0.0.0, ::), link-local addresses (169.254.0.0/16, fe80::/10), private RFC1918 addresses (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16), and port 0. IPv4-mapped IPv6 addresses MUST be normalised to IPv4 form before deduplication.
- **FR-014**: The system MUST limit the number of addresses accepted from a single peer-sharing response to the `amount` value sent in the corresponding `MsgShareRequest`. Entries beyond this limit MUST be discarded without decoding or allocating memory for them. This bound prevents memory amplification from malicious peers that ignore the requested amount and return an unbounded response. The `amount` requested per exchange is `target-peer-count` (default 15, u8 capped at 255).

### Key Entities

- **Hot Peer**: An actively connected peer running ChainSync and BlockFetch mini-protocols. Contributes directly to block synchronisation.
- **Cold Peer**: A known peer address with no active connection. Held in reserve for promotion when needed.
- **Peer-Sharing Exchange**: A short-lived connection to a hot peer used solely to request a list of additional peer addresses via the peer-sharing mini-protocol.
- **Churn Event**: A scheduled demotion of a randomly selected hot peer to cold, followed by promotion of a cold peer to restore connectivity.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A node configured with 3 peers recovers from losing 2 of them and resumes block synchronisation within 60 seconds, without operator intervention.
- **SC-002**: A node started with 3 configured peers and `target-peer-count = 15` accumulates at least 10 known peer addresses within 10 minutes of runtime on mainnet or preprod.
- **SC-003**: Over a 1-hour run with churn enabled, at least one hot peer replacement occurs per `churn-interval-secs` when cold peers are available.
- **SC-004**: Enabling P2P discovery produces no regression in block synchronisation throughput compared to the static peer configuration baseline.
- **SC-005**: When `peer-sharing-enabled = false`, node behaviour is identical to the pre-feature baseline with zero observable difference for operators who opt out.

## Clarifications

### Session 2026-03-09

- Q: Should the cold peer list survive node restarts? → A: In-memory only — cold list is rebuilt from config and peer-sharing on each restart.
- Q: How should peer discovery activity be observable? → A: Structured log lines only — log peer promotions, demotions, discoveries, and current hot/cold counts.
- Q: Should the cold peer list have an upper bound? → A: Cap at 4 × target-peer-count (e.g. 60 by default) — no extra config needed.
- Q: How should peer-sharing connection attempts be time-bounded? → A: Configurable via `peer-sharing-timeout-secs` config key (default 10 seconds), covering the full exchange.
- Q: Should peer-sharing requests be rate-limited? → A: Yes — hardcoded 5-minute per-peer cooldown; the same peer will not be queried again within 5 minutes of the last exchange.
- Q: From where do peers come? What discovery sources are used? → A: Two sources only: (1) static `node-addresses` from config seeds the cold list on startup; (2) the peer-sharing mini-protocol asks connected hot peers for their known addresses at runtime. No ledger peers, DNS bootstrap, or topology files.

## Assumptions

- Peer addresses come from exactly two sources: static `node-addresses` config (startup seed) and the peer-sharing mini-protocol (runtime discovery from hot peers). Ledger peers, DNS bootstrap, and topology files are explicitly out of scope.
- The cold peer list is in-memory only and is not persisted to disk. On restart, the node re-seeds from `node-addresses` and rebuilds the cold list via peer-sharing.
- Peer-sharing is implemented as a separate short-lived TCP connection to the target peer (not multiplexed onto the existing hot connection), since pallas 0.34 does not support registering additional mini-protocol channels after the multiplexer is spawned.
- The "warm" tier from the full Ouroboros governor is collapsed: all active connections are effectively hot (running ChainSync + BlockFetch). A separate warm tier is deferred.
- Round-trip time measurement and byte counting (connection instrumentation) are explicitly out of scope. Peer selection for churn and discovery uses random selection, not quality-based scoring.
- IPv4 addresses returned by peer-sharing are fully supported. IPv6 is handled if the codec returns it but is not a primary test target.
- Inbound connections (acting as a server to other nodes) are out of scope for this feature.
- Per L001: rollback scenarios do not affect the peer discovery lifecycle — peer state transitions are independent of chain events.
- Per L002: peer discovery is internal to the PNI module; no new inter-module messages are required.
