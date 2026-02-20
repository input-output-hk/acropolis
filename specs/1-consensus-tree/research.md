# Research: Consensus Tree

**Phase 0 output for feature 1-consensus-tree**

**Sources**:
- `docs/architecture/consensus-tree.md` — tree data structure design
- `docs/architecture/system-multi-peer-consensus.md` — multi-peer
  consensus system design (message flows, validation loop)
- `refs/pdf/Ouroboros Praos.txt` — chain selection rules

## Decision 1: Module placement

**Decision**: Create the ConsensusTree as a standalone library crate
inside `modules/consensus/src/`, not a separate workspace member. The
existing `acropolis_module_consensus` module will own and wire it to
the message bus.

**Rationale**: The architecture doc and spec both state the tree is a
library that does not subscribe to the bus directly. The existing
consensus module (`modules/consensus/src/consensus.rs`) already has
`// TODO Actually decide on favoured chain!` at line 85 — the tree
fills exactly that gap. Adding a new workspace crate would be
over-engineering for what is an internal data structure of the
consensus module.

**Alternatives considered**:
- Separate workspace crate (`modules/consensus_tree/`): Rejected —
  adds build and dependency overhead for a single-consumer library.
  Can always be extracted later if reuse demand emerges.
- Place in `common/`: Rejected — the tree is consensus-specific, not
  a general-purpose utility.

## Decision 2: Hash type

**Decision**: Use `BlockHash` from `acropolis_common::types` (which is
`Hash<32>`, a 32-byte fixed-size hash). It already implements `Eq`,
`Hash`, `Clone`, `Copy`, `Debug`, `Display`, and serde traits.

**Rationale**: `BlockHash` is used throughout the codebase for block
identification. The `BlockOfferedMessage` already provides `hash` and
`parent_hash` as `BlockHash`.

**Alternatives considered**: None — this is the canonical type.

## Decision 3: Block representation in the tree

**Decision**: Create a `TreeBlock` struct (internal to consensus_tree)
containing:
- `hash: BlockHash`
- `number: u64` (block height)
- `slot: u64` (slot number)
- `body: Option<Vec<u8>>` (raw block body, None until fetched)
- `parent: Option<BlockHash>` (genesis root has None)
- `children: Vec<BlockHash>` (child block hashes)
- `status: BlockValidationStatus` (see Decision 7)

Use `HashMap<BlockHash, TreeBlock>` for O(1) lookup. Store parent and
child references as `BlockHash` keys, not `Arc<Block>` pointers.

**Rationale**: Using hash keys instead of `Arc` pointers avoids
reference cycles (parent ↔ child) which would prevent memory
reclamation and complicate removal. The `HashMap` already provides O(1)
access. This matches the architecture doc's intent while avoiding the
`Arc` cycle problem it would introduce.

**Alternatives considered**:
- `Arc<Block>` pointers as in the architecture doc sketch: Rejected —
  creates parent-child reference cycles requiring `Weak` references
  or manual cycle breaking. Hash-based indirection through the
  `HashMap` is simpler and equally performant at k=2160 scale.
- Arena allocation (e.g., `slotmap`): Rejected — adds a dependency for
  no clear benefit at this scale.

## Decision 4: Observer/callback mechanism

**Decision**: Use a trait-based observer pattern. Define a
`ConsensusTreeObserver` trait with methods:
- `block_proposed(&self, ...)` — block ready for validation/apply
- `rollback(&self, ...)` — favoured chain switched
- `block_rejected(&self, ...)` — block failed validation, notify PNI

The tree takes a boxed observer at construction time.

**Rationale**: Trait-based observers are idiomatic Rust, testable (mock
implementations in tests), and avoid the complexity of closures
capturing external state. The system design doc
(`system-multi-peer-consensus.md`) defines three outbound flows from
consensus: proposed, rollback, and rejected.

**Alternatives considered**:
- Closures (`Box<dyn Fn(...)>`): Viable but less structured; harder
  to mock and name in test assertions.
- Channel-based (`tokio::sync::mpsc`): Rejected — the tree is
  synchronous per the spec (single-threaded, owning module handles
  concurrency). Channels add unnecessary async complexity.
- Return events from operations: Viable alternative — operations
  return a `Vec<TreeEvent>` enum and the caller dispatches. Simpler
  but moves dispatch logic to the caller. Could be reconsidered if
  trait approach proves awkward.

## Decision 5: Bounded maxvalid (k-block fork limit)

**Decision**: Enforce the bounded `maxvalid` rule in
`check_block_wanted`. When a new block is inserted, if the fork point
from the current favoured chain is deeper than k blocks, reject the
block.

**Rationale**: The Praos paper (line 1798-1800) defines maxvalid for
the dynamic-stake protocol as rejecting chains that fork more than k
blocks. This is enforced at insertion time rather than at selection
time, which is more efficient (no need to walk the full tree on every
selection).

**Alternatives considered**:
- Enforce at selection time (in `get_favoured_chain`): Less efficient;
  would require computing fork depth for every candidate chain on
  every call.
- Accept and ignore (let pruning handle it): Incorrect — a chain
  forking >k blocks from current should never become favoured, even
  transiently.

## Decision 6: Error handling

**Decision**: Define a `ConsensusTreeError` enum using `thiserror`:
- `ParentNotFound { hash: BlockHash }`
- `InvalidBlockNumber { expected: u64, got: u64 }`
- `BlockNotInTree { hash: BlockHash }`
- `ForkTooDeep { fork_depth: u64, max_k: u64 }`
- `ValidationFailed { hash: BlockHash }`

Operations return `Result<T, ConsensusTreeError>`.

**Rationale**: Constitution Principle I mandates typed errors via
`thiserror`, no `unwrap()` or `panic!()` in library code.

**Alternatives considered**: `anyhow::Error` — rejected for a library;
`thiserror` provides caller-matchable error variants.

## Decision 7: Block validation status tracking

**Decision**: Each `TreeBlock` tracks its validation status via a
`BlockValidationStatus` enum:
- `Offered` — block header known, not yet fetched (on unfavoured fork)
- `Wanted` — block has been requested for fetching (on favoured chain)
- `Fetched` — block body received, awaiting validation
- `Validated` — block passed validation
- `Rejected` — block failed validation

**Rationale**: The system design doc (`system-multi-peer-consensus.md`,
line 100-102) specifies that blocks on unfavoured forks are tracked but
not fetched or validated. Only blocks on the favoured chain are
requested, fetched, and sent for validation. When the favoured chain
switches, previously unfavoured blocks need to be fetched and
validated. The tree must track where each block is in this lifecycle
to determine what actions are needed.

**Alternatives considered**:
- Boolean `validated` flag: Too simplistic — doesn't distinguish
  between "not yet fetched" and "fetched but awaiting validation".
- No status tracking (let caller manage): Rejected — the tree needs
  status to decide which blocks to request on chain switches and
  which blocks to propose.

## Decision 8: Fetch-only-favoured strategy

**Decision**: `check_block_wanted` only returns hashes for blocks that
are on (or extend) the currently favoured chain. Blocks offered on
unfavoured forks are added to the tree as `Offered` but are NOT
requested for fetching.

When a chain switch occurs (new fork becomes longest), the tree walks
the new favoured chain from the common ancestor to the tip and returns
any `Offered` (unfetched) blocks as newly wanted.

**Rationale**: Per `system-multi-peer-consensus.md` line 100-102:
"If the new block is on another, unfavoured, fork, we don't fetch,
or validate it yet, but add it marked as unvalidated to the relevant
chain in the tree." This avoids wasting bandwidth fetching blocks on
forks that may never become favoured.

**Alternatives considered**:
- Fetch all offered blocks eagerly: Rejected — wastes bandwidth and
  validation resources on blocks that may never be needed.
- Don't track unfavoured blocks at all: Rejected — we need them in
  the tree to detect when an unfavoured fork becomes longest and
  triggers a chain switch.

## Decision 9: Validation feedback loop

**Decision**: The tree exposes two feedback operations:
- `mark_validated(hash)` — called when validation succeeds. Updates
  status to `Validated`.
- `mark_rejected(hash)` — called when validation fails. Updates status
  to `Rejected`, removes the block and its descendants from the tree,
  fires `block_rejected` observer, and handles any resulting chain
  switch (the rejected chain may no longer be the longest).

**Rationale**: Per `system-multi-peer-consensus.md` line 91-95:
Consensus sends blocks for validation and acts on the result. If
validation fails, the chain is truncated (line 116-118), which may
cause a chain switch. The `block_rejected` observer maps to the
`cardano.block.rejected` message that PNI uses to sanction peers.

**Alternatives considered**:
- Handle validation entirely in the consensus module (outside the
  tree): Viable but duplicates chain-switch logic that already exists
  in `remove_block`. Better to have the tree handle truncation
  internally for consistency.
