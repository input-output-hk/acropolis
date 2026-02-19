# Feature Specification: Consensus Tree Data Structure

**Feature Branch**: `prc/consensus-tree-doc`
**Created**: 2026-02-17
**Status**: Draft
**Input**: User description: "Implement the ConsensusTree data structure as described in docs/architecture/consensus-tree.md"

**Reference material**:
- `docs/architecture/consensus-tree.md` — Acropolis design for the tree
- `docs/architecture/system-multi-peer-consensus.md` — multi-peer
  consensus system design (validation loop, fetch-only-favoured strategy)
- `refs/pdf/Ouroboros Praos.txt` — Praos paper, defines `maxvalid` and
  Common Prefix property
- `refs/notes/chain_selection.md` — curated chain selection rules
- `refs/notes/invariants.md` — consensus invariants

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Track Chain Forks in Volatile Window (Priority: P1)

As a node operator, I need the system to maintain a tree of all
potentially viable chain forks within the volatile window (k blocks
from tip) so that the node can identify and follow the favoured chain
at all times.

The favoured chain is selected by the `maxvalid` rule defined in the
Praos paper: return the longest valid chain, with ties broken in
favour of the current chain (Praos paper, line 667-668). The bounded
variant (paper, line 1798-1800) further restricts selection to chains
that do not fork from the current chain by more than k blocks.

**Why this priority**: This is the foundational data structure. Without
the tree and favoured-chain selection, no other consensus operation
can function.

**Independent Test**: Can be fully tested by inserting blocks into
the tree in various fork patterns and verifying that favoured chain
selection always returns the tip of the longest valid branch, and
rejects chains forking deeper than k blocks.

**Acceptance Scenarios**:

1. **Given** an empty tree with a genesis root, **When** a linear
   sequence of blocks is added, **Then** the favoured chain tip is
   the most recently added block.
2. **Given** a tree with a fork at block N, **When** one branch grows
   longer than the other, **Then** favoured chain selection returns
   the tip of the longer branch.
3. **Given** two branches of equal length, **When** no new block
   arrives, **Then** the existing favoured tip is retained (no switch,
   no rollback). Per Praos: ties broken in favour of current chain.
4. **Given** a candidate chain that forks from the current chain more
   than k blocks back, **Then** it is rejected by the bounded
   `maxvalid` rule and does not become the favoured chain.

---

### User Story 2 - Receive and Request Blocks from Peers (Priority: P1)

As a node operator, I need the system to evaluate blocks offered by
peers, decide which blocks are wanted, and incorporate fetched block
bodies so the node can stay synchronised with the network.

**Why this priority**: This is the primary ingestion path — the
operations `check_block_wanted` and `add_block` are the main interface
between the peer network and consensus tracking.

**Independent Test**: Can be tested by simulating peer block offers
(header only) via `check_block_wanted`, then delivering block bodies
via `add_block`, and verifying that observers fire in correct order.

**Acceptance Scenarios**:

1. **Given** a tree with tip at block N, **When** a peer offers
   block N+1 with valid parent hash, **Then** the block is added to
   the tree (without body) and its hash is returned as wanted.
2. **Given** a block in the tree without a body, **When** the block
   body arrives via `add_block`, **Then** the body is stored and
   `block_proposed` fires for that block and any subsequent fetched
   blocks on the favoured chain.
3. **Given** a peer offers a block whose parent is not in the tree,
   **Then** the operation returns an error and the block is not added.
4. **Given** a peer offers a block whose number is not exactly
   parent's number + 1, **Then** the operation returns an error.

---

### User Story 3 - Detect and Signal Rollbacks (Priority: P1)

As a node operator, I need the system to detect when the favoured
chain switches to a different fork and signal a rollback to the
common ancestor so that downstream state modules can revert
appropriately.

**Why this priority**: Rollback detection is critical for correctness.
Without it, state modules would apply blocks from a stale fork,
leading to an inconsistent ledger. Per the Praos Common Prefix
property (paper, line 310-313), rollbacks beyond k blocks should
never occur under honest-majority conditions.

**Independent Test**: Can be tested by building a tree with two forks,
making the shorter fork become longer, and verifying that the
`rollback` observer fires with the correct common ancestor block
number.

**Acceptance Scenarios**:

1. **Given** the favoured chain is A->B->C->D, **When** a new block E
   is added making fork A->B->F->G->E the longest chain, **Then** the
   `rollback` observer is called with block B's number (the common
   ancestor).
2. **Given** a rollback has occurred, **Then** `block_proposed` fires
   for each already-fetched block on the new favoured chain from the
   common ancestor forward, in order.
3. **Given** a block is removed via `remove_block` causing a chain
   switch, **Then** the rollback observer fires and the system returns
   newly wanted block hashes for unfetched blocks on the new favoured
   chain.
4. **Given** a fetched block passes validation, **When**
   `mark_validated` is called, **Then** its status transitions to
   Validated.
5. **Given** a fetched block fails validation, **When**
   `mark_rejected` is called, **Then** the `block_rejected` observer
   fires, the block and all its descendants are removed, and if the
   truncated chain is no longer longest, a chain switch occurs
   (ref: `system-multi-peer-consensus.md` line 116-118).

---

### User Story 4 - Prune Immutable Blocks (Priority: P2)

As a node operator, I need the system to discard blocks that are
deeper than k blocks from the tip so that memory usage remains bounded
during long-running operation.

The Praos Common Prefix property guarantees (with overwhelming
probability) that the (k+1)th block from the tip will never be rolled
back. Blocks beyond that depth are immutable and can be safely pruned.

**Why this priority**: Important for production stability but not
required for basic consensus tracking to function. The tree would
simply grow without bound until pruning is implemented.

**Independent Test**: Can be tested by adding more than k+1 blocks
to a linear chain and verifying that blocks older than tip minus k
are removed, and that non-favoured branches rooted before the prune
point are also removed.

**Acceptance Scenarios**:

1. **Given** a linear chain longer than k blocks, **When** pruning
   runs, **Then** all blocks with number less than (tip - k) are
   removed from the tree and the hash map.
2. **Given** a fork that diverges before the prune boundary, **When**
   pruning runs, **Then** the entire non-favoured branch is removed.
3. **Given** a fork that diverges after the prune boundary, **When**
   pruning runs, **Then** both branches are preserved.

---

### Edge Cases

- What happens when two forks are exactly the same length?
  The current tip remains favoured (no switch, no rollback).
  Per Praos: "Ties are broken in favor of C, if it has maximum
  length" (paper, line 667-668).
- What happens when `add_block` is called with a hash not in the tree?
  Returns an error — only blocks previously registered via
  `check_block_wanted` can receive bodies.
- What happens when `add_block` is called for a block that already has
  a body? The operation is a no-op (idempotent).
- What happens when `remove_block` is called for the root block?
  The root and all children are removed, leaving an empty tree.
- What happens when blocks arrive out of order on the favoured chain?
  `block_proposed` fires only up to the first gap — remaining blocks
  are proposed when the gap is filled.
- What happens when a candidate chain forks deeper than k blocks?
  It is rejected by the bounded maxvalid rule (paper, line 1798-1800).
  This enforces the Common Prefix guarantee at the data structure
  level.
- What happens when a block is offered on an unfavoured fork?
  It is added to the tree with status Offered but is NOT returned
  as wanted and is NOT fetched. If a later chain switch makes that
  fork favoured, the block transitions to Wanted and is returned
  for fetching (ref: `system-multi-peer-consensus.md` line 100-102).
- What happens when a validated block's chain is later rejected?
  If `mark_rejected` is called on a block, all descendants (including
  any with Validated status) are removed. The chain is truncated,
  potentially triggering a chain switch.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST store blocks in a tree structure keyed by
  block hash, supporting O(1) lookup.
- **FR-002**: Each block MUST track its height, slot, optional raw
  body, child pointers, parent pointer, and validation lifecycle
  status.
- **FR-003**: System MUST determine the favoured (longest) chain by
  recursive traversal from the root, implementing the Praos
  `maxvalid` rule: select the longest valid chain, with ties broken
  in favour of the current chain.
- **FR-004**: System MUST enforce the bounded `maxvalid` rule: reject
  any candidate chain that forks from the current chain by more than
  k blocks (Praos paper, line 1798-1800).
- **FR-005**: System MUST detect favoured-chain switches and compute
  the common ancestor between old and new tips.
- **FR-006**: System MUST support observer callbacks for
  `block_proposed`, `rollback`, and `block_rejected` events.
  `block_rejected` notifies PNI to sanction peers that provided
  invalid blocks (ref: `system-multi-peer-consensus.md`, line 67-68).
- **FR-007**: `check_block_wanted` MUST validate that the offered
  block's number equals parent number + 1, reject otherwise.
- **FR-008**: `check_block_wanted` MUST return a list of wanted block
  hashes (the offered block plus any unfetched blocks on the new
  favoured chain).
- **FR-009**: `add_block` MUST store the block body and fire
  `block_proposed` for contiguous fetched blocks on the favoured
  chain starting from the earliest unproposed block.
- **FR-010**: `remove_block` MUST remove the target block and all its
  descendants, then handle any resulting chain switch.
- **FR-011**: `prune` MUST remove all blocks with number less than
  (latest_number - k), and recursively remove non-favoured branches
  rooted before the prune boundary.
- **FR-012**: The security parameter k MUST be configurable
  (default: 2160, per Praos Common Prefix with parameter k).
- **FR-013**: Chain selection MUST be deterministic and pure — no
  access to mempool, network state, or randomness
  (ref: `refs/notes/invariants.md`, `refs/notes/chain_selection.md`).
- **FR-014**: Each block in the tree MUST track its lifecycle status
  through: Offered (header known, on unfavoured fork, not fetched),
  Wanted (fetch requested, on favoured chain), Fetched (body received,
  awaiting validation), Validated (passed validation), or Rejected
  (failed validation, immediately removed). Transitions: Offered →
  Wanted (on chain switch), Wanted → Fetched (on body arrival),
  Fetched → Validated or Rejected. Ref:
  `system-multi-peer-consensus.md` line 100-102.
- **FR-015**: `check_block_wanted` MUST assign status Wanted to blocks
  extending the favoured chain and status Offered to blocks on
  unfavoured forks. Only Wanted block hashes are returned for fetching.
  Ref: `system-multi-peer-consensus.md` line 100-102.
- **FR-016**: System MUST provide `mark_validated(hash)` to transition
  a Fetched block to Validated status, confirming it passed validation.
- **FR-017**: System MUST provide `mark_rejected(hash)` to handle
  validation failure: fire `block_rejected` observer, remove the block
  and all its descendants, and handle any resulting chain switch
  (the truncated chain may no longer be longest). Ref:
  `system-multi-peer-consensus.md` line 116-118.

### Key Entities

- **TreeBlock**: A node in the consensus tree. Contains block hash
  (identity key), block number (height), slot number, optional raw
  body, parent pointer, child pointers, and validation lifecycle
  status (BlockValidationStatus). Identified by its hash.
- **BlockValidationStatus**: Enum tracking where a block is in the
  fetch-validate lifecycle: Offered, Wanted, Fetched, Validated,
  Rejected (see FR-014).
- **ConsensusTree**: The top-level data structure. Holds the root
  block pointer, a hash map of all blocks, the current favoured
  tip, the security parameter k, and an observer callback receiver.
  Exposes all public operations.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The favoured chain is correctly identified after every
  insertion, removal, or body delivery — verified by automated tests
  covering at least 10 distinct fork topologies.
- **SC-002**: Rollback observers fire with the correct common ancestor
  in every chain-switch scenario — verified by tests with single and
  multi-level rollbacks.
- **SC-003**: Block proposed observers fire in strictly ascending
  block-number order, with no gaps and no duplicates.
- **SC-004**: After pruning, no block older than (tip - k) remains
  in the tree, and memory usage is bounded proportionally to k.
- **SC-005**: All operations handle error cases (unknown parent,
  invalid height, missing block) without panicking — returning
  typed errors instead.
- **SC-006**: Candidate chains forking deeper than k blocks from the
  current chain are rejected — verified by tests that attempt to
  insert deep-forking chains and confirm they do not become favoured.
- **SC-007**: Chain selection is deterministic — the same sequence of
  operations always produces the same favoured tip, regardless of
  execution timing.
- **SC-008**: Block validation lifecycle (Offered → Wanted → Fetched →
  Validated) is correctly tracked — verified by tests covering status
  transitions, chain-switch-triggered Offered→Wanted promotion, and
  rejection-triggered removal with chain truncation.

### Assumptions

- The ConsensusTree is a standalone data structure (library) that
  does not directly subscribe to the message bus. A separate module
  will own the tree and wire it to message bus topics.
- Block hashes are provided externally (decoded from CBOR by other
  modules); the tree does not perform hashing.
- The tree operates in a single-threaded context (the owning module
  handles concurrency). Internal locking is not required.
- Tie-breaking for equal-length forks: the existing favoured tip is
  retained (no switch, no rollback). This matches the Praos
  `maxvalid` definition (paper, line 667-668).
- The bounded maxvalid k-block fork limit (FR-004) uses the same k
  as the pruning parameter. This is consistent with the Praos paper
  where k serves both roles.
- No explicit per-operation latency targets are defined. The tree
  holds at most ~k blocks (~2160) in memory, so operations should
  be efficient at that scale. Implementations MUST NOT introduce
  gratuitous inefficiency (e.g., full-tree scans where a hash lookup
  suffices). Performance concerns are deferred to profiling during
  implementation.

## Clarifications

### Session 2026-02-17

- Q: Should the spec define performance targets for tree operations?
  → A: Defer explicit targets — scale is small enough. But
  implementations must not be gratuitously inefficient.
