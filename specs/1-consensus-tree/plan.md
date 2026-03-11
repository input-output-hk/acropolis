# Implementation Plan: Consensus Tree

**Branch**: `prc/consensus-tree-doc` | **Date**: 2026-02-17 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/1-consensus-tree/spec.md`

**Sources**:
- `docs/architecture/consensus-tree.md` — tree data structure design
- `docs/architecture/system-multi-peer-consensus.md` — multi-peer
  consensus system design (message flows, validation loop)
- `refs/pdf/Ouroboros Praos.txt` — chain selection rules (`maxvalid`)

## Summary

Implement the ConsensusTree data structure as a library within the
existing `acropolis_module_consensus` crate. The tree tracks volatile
chain forks, selects the favoured (longest) chain per the Praos
`maxvalid` rule, manages a fetch-validate lifecycle for blocks, and
notifies observers of chain switches, block proposals, and validation
failures.

The tree participates in a validation feedback loop: blocks on the
favoured chain are fetched and sent for validation via observers.
Blocks on unfavoured forks are tracked but not fetched until a chain
switch makes them part of the new favoured chain. Validation failures
cause chain truncation and potentially trigger further chain switches.

This replaces the `// TODO Actually decide on favoured chain!`
placeholder at line 85 of the existing consensus module.

## Technical Context

**Language/Version**: Rust 2021 edition
**Primary Dependencies**: `acropolis_common` (BlockHash, BlockInfo,
  ConsensusMessage types), `thiserror`
**Storage**: In-memory HashMap (bounded by k ~ 2160 entries)
**Testing**: `cargo test -p acropolis_module_consensus`
**Target Platform**: Linux server (same as Acropolis)
**Project Type**: Library within existing module
**Performance Goals**: No explicit latency targets; must not be
  gratuitously inefficient at k=2160 scale
**Constraints**: No panics, no unsafe, clippy clean, all public
  items documented
**Scale/Scope**: ~2160 blocks in memory, single-threaded access

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked post-design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Quality & Safety | PASS | thiserror for errors, no unwrap/panic, clippy clean, doc comments on all public items |
| II. Testing Standards | PASS | TDD workflow, unit tests per public API, descriptive test names, no external dependencies |
| III. Interface & Experience Consistency | PASS | No new message types needed; existing ConsensusMessage variants (BlockOffered, BlockRescinded, BlockWanted, BlockRejected) already defined in common/src/messages.rs. Observer trait is the internal interface boundary. |
| IV. Performance & Reliability | PASS | Bounded memory (k blocks), no blocking I/O, deterministic chain selection |

No violations. No complexity tracking entries needed.

## Project Structure

### Documentation (this feature)

```text
specs/1-consensus-tree/
├── spec.md              # Feature specification
├── plan.md              # This file
├── research.md          # Phase 0: design decisions (9 decisions)
├── data-model.md        # Phase 1: entity model + message flow
├── quickstart.md        # Phase 1: usage guide
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code

```text
modules/consensus/
├── Cargo.toml                    # Add thiserror dependency
└── src/
    ├── consensus.rs              # Existing lib entry point (rewire to use tree)
    ├── consensus_tree.rs         # NEW: ConsensusTree struct + operations
    ├── tree_block.rs             # NEW: TreeBlock + BlockValidationStatus
    ├── tree_error.rs             # NEW: ConsensusTreeError enum
    └── tree_observer.rs          # NEW: ConsensusTreeObserver trait
```

**Structure Decision**: Library files added to existing
`modules/consensus/` crate. No new workspace member. No `lib.rs`
needed — `Cargo.toml` already sets `[lib] path = "src/consensus.rs"`,
which is the convention across all Acropolis modules. New sub-modules
are declared via `mod` items in `consensus.rs`. The consensus module
already depends on `acropolis_common` which provides `BlockHash` and
the `ConsensusMessage` types.

## Design Decisions Summary

(Full rationale in [research.md](research.md))

1. **Module placement**: Library inside `modules/consensus/`, not a
   separate crate.
2. **Hash type**: `BlockHash` from `acropolis_common`.
3. **Block references**: Hash-based indirection via HashMap, not
   `Arc` pointers (avoids reference cycles).
4. **Observer pattern**: Trait-based (`ConsensusTreeObserver`) with
   three callbacks: `block_proposed`, `rollback`, `block_rejected`.
5. **Bounded maxvalid**: Enforced at insertion time in
   `check_block_wanted`.
6. **Error handling**: `ConsensusTreeError` enum via `thiserror`.
7. **Validation status**: `BlockValidationStatus` enum tracks each
   block through Offered → Wanted → Fetched → Validated lifecycle.
8. **Fetch-only-favoured**: Only blocks on the favoured chain are
   requested for fetching. Unfavoured fork blocks are tracked as
   `Offered` until a chain switch.
9. **Validation feedback**: `mark_validated()` and `mark_rejected()`
   close the validation loop. Rejection triggers chain truncation.

## Message Flow

Per `system-multi-peer-consensus.md`, the consensus module wires
tree operations to these bus topics:

| Bus topic                | Direction | Tree operation          |
|--------------------------|-----------|-------------------------|
| `cardano.block.offered`  | PNI → CON | `check_block_wanted()`  |
| `cardano.block.wanted`   | CON → PNI | return value of above   |
| `cardano.block.available`| PNI → CON | `add_block()`           |
| `cardano.block.proposed` | CON → VAL | observer: block_proposed|
| `cardano.validation.*`   | VAL → CON | `mark_validated/rejected()` |
| `cardano.block.rejected` | CON → PNI | observer: block_rejected|
| `cardano.block.rescinded`| PNI → CON | `remove_block()`        |

The tree itself does not subscribe to any topics. The consensus
module translates between bus messages and tree operations.

## Implementation Phases

### Phase 1: Core data structures

- `BlockValidationStatus` enum (Offered, Wanted, Fetched, Validated,
  Rejected)
- `TreeBlock` struct with all fields including status
- `ConsensusTreeError` enum via thiserror
- `ConsensusTreeObserver` trait (block_proposed, rollback,
  block_rejected)
- `ConsensusTree` struct with `new()` and `set_root()`

### Phase 2: Chain selection helpers

- `get_favoured_chain()` — recursive longest-chain from root
  (Praos maxvalid: longest chain, ties favour current)
- `find_common_ancestor()` — walk-back from two tips
- `chain_contains()` — check if block is on chain ending at tip
- `fork_depth()` — compute how deep a fork diverges from current
  chain (for bounded maxvalid enforcement)

### Phase 3: Block ingestion operations

- `check_block_wanted(hash, parent_hash, number, slot)`:
  - Validate parent exists and number = parent + 1
  - Enforce bounded maxvalid (fork depth ≤ k)
  - Insert block as `Offered` (unfavoured) or `Wanted` (favoured)
  - If new block makes a different fork longest: detect chain switch,
    fire rollback observer, transition Offered blocks on new favoured
    chain to Wanted, fire block_proposed for already-fetched blocks
  - Return list of Wanted block hashes
- `add_block(body)`:
  - Decode hash from body, find block in tree
  - Store body, transition status to Fetched
  - Fire block_proposed for contiguous Fetched blocks on favoured
    chain from earliest unproposed

### Phase 4: Validation feedback + removal

- `mark_validated(hash)` — transition status to Validated
- `mark_rejected(hash)` — fire block_rejected observer, remove block
  and all descendants, handle potential chain switch (truncated chain
  may no longer be longest)
- `remove_block(hash)` — remove block and descendants (for
  `cardano.block.rescinded`), handle chain switch, return newly
  wanted hashes

### Phase 5: Pruning

- `prune()` — remove blocks older than (tip - k), clean dead forks
  using chain_contains to identify favoured vs non-favoured branches

### Phase 6: Integration with consensus module

- Add `thiserror` dependency to `modules/consensus/Cargo.toml`
- Add `mod` declarations and re-exports in `consensus.rs` (the
  existing lib entry point per `[lib] path = "src/consensus.rs"`)
- Rewire `consensus.rs`:
  - Replace passthrough logic with ConsensusTree instance
  - Subscribe to `cardano.block.offered` (new) in addition to
    `cardano.block.available` (existing)
  - Subscribe to `cardano.block.rescinded` (new)
  - Implement `ConsensusTreeObserver` to publish to bus:
    - `block_proposed` → publish `cardano.block.proposed`
    - `rollback` → publish rollback state transition
    - `block_rejected` → publish `cardano.block.rejected`
  - Wire validation responses back via `mark_validated/rejected`
  - Add `k` to module configuration (TOML key: `security-parameter`,
    default 2160)

### Phase 7: Tests

Written TDD-style alongside each phase, but listed here for
completeness:

- **Unit tests per operation** (Phases 1-5):
  - Tree construction and root setting
  - get_favoured_chain for 10+ fork topologies (SC-001)
  - find_common_ancestor correctness
  - chain_contains edge cases
  - fork_depth computation
  - check_block_wanted: favoured vs unfavoured insertion, chain
    switch detection, bounded maxvalid rejection
  - add_block: body storage, block_proposed ordering (SC-003),
    out-of-order delivery
  - mark_validated / mark_rejected: status transitions, chain
    truncation on rejection
  - remove_block: descendant removal, chain switch
  - prune: boundary correctness, dead fork cleanup (SC-004)
  - Error cases: unknown parent, invalid height, missing block,
    deep fork (SC-005, SC-006)
  - Determinism (SC-007)
- **Rollback tests** (SC-002): single-level, multi-level, rollback
  after rejection
- **Integration tests** (Phase 6): end-to-end with mock bus

## Complexity Tracking

No constitution violations to justify.
