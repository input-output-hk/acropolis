# Tasks: Consensus Tree Data Structure

**Input**: Design documents from `specs/1-consensus-tree/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md

**Tests**: Included — constitution Principle II mandates TDD workflow.

**Organization**: Tasks are grouped by user story to enable
independent implementation and testing of each story. US1-US3 are
all P1 but have natural dependencies (US1 provides the foundation
for US2 and US3).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3, US4)
- Include exact file paths in descriptions

---

## Phase 1: Setup

**Purpose**: Project structure and shared types

- [x] T001 Add `thiserror` dependency to modules/consensus/Cargo.toml
- [x] T002 [P] Create BlockValidationStatus enum (Offered, Wanted, Fetched, Validated, Rejected) in modules/consensus/src/tree_block.rs
- [x] T003 [P] Create ConsensusTreeError enum with thiserror derives (ParentNotFound, InvalidBlockNumber, BlockNotInTree, ForkTooDeep, ValidationFailed) in modules/consensus/src/tree_error.rs
- [x] T004 [P] Create ConsensusTreeObserver trait with block_proposed(), rollback(), block_rejected() methods in modules/consensus/src/tree_observer.rs
- [x] T005 Create TreeBlock struct (hash, number, slot, body, parent, children, status) in modules/consensus/src/tree_block.rs
- [x] T006 Create ConsensusTree struct with new(k, observer) and set_root() in modules/consensus/src/consensus_tree.rs
- [x] T007 Add mod declarations and re-exports for tree modules in modules/consensus/src/consensus.rs (existing lib entry point per Cargo.toml `[lib] path`)

**Checkpoint**: Project compiles (`cargo build -p acropolis_module_consensus`). All types defined.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Chain selection helpers that ALL user stories depend on

**CRITICAL**: No user story work can begin until this phase is complete

### Tests for Foundational

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [x] T008 [P] Test get_favoured_chain returns root tip for single-block tree in modules/consensus/src/consensus_tree.rs
- [x] T009 [P] Test get_favoured_chain returns longer branch tip for forked tree in modules/consensus/src/consensus_tree.rs
- [x] T010 [P] Test get_favoured_chain retains current tip on equal-length forks (Praos tie-break) in modules/consensus/src/consensus_tree.rs
- [x] T011 [P] Test find_common_ancestor returns correct ancestor for two diverging tips in modules/consensus/src/consensus_tree.rs
- [x] T012 [P] Test chain_contains returns true for block on chain, false otherwise in modules/consensus/src/consensus_tree.rs
- [x] T013 [P] Test fork_depth returns correct depth for various fork positions in modules/consensus/src/consensus_tree.rs

### Implementation for Foundational

- [x] T014 Implement get_favoured_chain() — recursive longest-chain from root, ties favour current tip in modules/consensus/src/consensus_tree.rs
- [x] T015 Implement find_common_ancestor() — walk-back from two tips to shared block in modules/consensus/src/consensus_tree.rs
- [x] T016 [P] Implement chain_contains() — check if block is on chain ending at tip in modules/consensus/src/consensus_tree.rs
- [x] T017 [P] Implement fork_depth() — compute fork divergence depth from current chain in modules/consensus/src/consensus_tree.rs

**Checkpoint**: All helpers pass tests. Foundation ready for user story phases.

---

## Phase 3: User Story 1 - Track Chain Forks in Volatile Window (Priority: P1) MVP

**Goal**: Maintain a tree of chain forks and select the favoured chain
using the Praos maxvalid rule, including bounded maxvalid (k-block
fork limit).

**Independent Test**: Insert blocks in various fork patterns; verify
get_favoured_chain always returns the longest valid branch tip and
rejects chains forking deeper than k blocks.

### Tests for User Story 1

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [x] T018 [P] [US1] Test 10+ fork topologies: linear, single fork, multi-fork, deep tree, balanced, skewed, chain-of-forks, diamond, zigzag, lopsided in modules/consensus/src/consensus_tree.rs (SC-001)
- [x] T019 [P] [US1] Test bounded maxvalid rejects block with fork depth > k in modules/consensus/src/consensus_tree.rs (SC-006)
- [x] T020 [P] [US1] Test determinism: same insertion sequence always produces same favoured tip in modules/consensus/src/consensus_tree.rs (SC-007)
- [x] T021 [P] [US1] Test error: block with unknown parent returns ParentNotFound in modules/consensus/src/consensus_tree.rs (SC-005)
- [x] T022 [P] [US1] Test error: block with invalid number returns InvalidBlockNumber in modules/consensus/src/consensus_tree.rs (SC-005)

### Implementation for User Story 1

- [x] T023 [US1] Implement check_block_wanted() — validate parent/number, enforce bounded maxvalid, insert block as Offered or Wanted, detect chain switch in modules/consensus/src/consensus_tree.rs (FR-003, FR-004, FR-007)

**Checkpoint**: User Story 1 fully functional and testable independently.

---

## Phase 4: User Story 2 - Receive and Request Blocks from Peers (Priority: P1)

**Goal**: Evaluate offered blocks, decide which to fetch (favoured
chain only), incorporate block bodies, and fire block_proposed
observers in correct order.

**Independent Test**: Simulate peer offers via check_block_wanted,
deliver bodies via add_block, verify observers fire in ascending
block-number order with no gaps.

### Tests for User Story 2

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [x] T024 [P] [US2] Test check_block_wanted returns hash as wanted when block extends favoured chain in modules/consensus/src/consensus_tree.rs
- [x] T025 [P] [US2] Test check_block_wanted does NOT return hash as wanted for block on unfavoured fork (status: Offered) in modules/consensus/src/consensus_tree.rs
- [x] T026 [P] [US2] Test add_block stores body and fires block_proposed for contiguous fetched blocks in modules/consensus/src/consensus_tree.rs (SC-003)
- [x] T027 [P] [US2] Test add_block with out-of-order delivery: block_proposed fires only up to first gap in modules/consensus/src/consensus_tree.rs
- [x] T028 [P] [US2] Test add_block for already-fetched block is idempotent (no-op) in modules/consensus/src/consensus_tree.rs
- [x] T029 [P] [US2] Test add_block for hash not in tree returns BlockNotInTree error in modules/consensus/src/consensus_tree.rs (SC-005)

### Implementation for User Story 2

- [x] T030 [US2] Implement add_block() — decode hash, store body, transition Wanted→Fetched, fire block_proposed for contiguous fetched blocks on favoured chain in modules/consensus/src/consensus_tree.rs (FR-008, FR-009)

**Checkpoint**: User Stories 1 AND 2 both work independently.

---

## Phase 5: User Story 3 - Detect and Signal Rollbacks (Priority: P1)

**Goal**: Detect favoured chain switches, signal rollbacks to common
ancestor, handle validation feedback (mark_validated/mark_rejected),
and handle block removal (rescinded).

**Independent Test**: Build tree with two forks, make shorter fork
longest, verify rollback fires with correct ancestor. Also test
rejection-triggered truncation and rescinded-block removal.

### Tests for User Story 3

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [x] T031 [P] [US3] Test chain switch fires rollback with correct common ancestor block number in modules/consensus/src/consensus_tree.rs (SC-002)
- [x] T032 [P] [US3] Test multi-level rollback (fork point several blocks back) fires correct ancestor in modules/consensus/src/consensus_tree.rs (SC-002)
- [x] T033 [P] [US3] Test rollback followed by block_proposed for fetched blocks on new favoured chain in order in modules/consensus/src/consensus_tree.rs (SC-003)
- [x] T034 [P] [US3] Test chain switch transitions Offered blocks on new favoured chain to Wanted and returns them in modules/consensus/src/consensus_tree.rs
- [x] T035 [P] [US3] Test mark_validated transitions status to Validated in modules/consensus/src/consensus_tree.rs
- [x] T036 [P] [US3] Test mark_rejected fires block_rejected, removes block + descendants, handles chain switch in modules/consensus/src/consensus_tree.rs
- [x] T037 [P] [US3] Test remove_block removes block and all descendants in modules/consensus/src/consensus_tree.rs
- [x] T038 [P] [US3] Test remove_block causing chain switch fires rollback and returns newly wanted hashes in modules/consensus/src/consensus_tree.rs

### Implementation for User Story 3

- [x] T039 [US3] Implement chain-switch detection and rollback signalling within check_block_wanted (extend T023) — fire rollback observer, transition Offered→Wanted on new chain, fire block_proposed for fetched blocks in modules/consensus/src/consensus_tree.rs (FR-005, FR-006)
- [x] T040 [P] [US3] Implement mark_validated() — transition Fetched→Validated in modules/consensus/src/consensus_tree.rs
- [x] T041 [P] [US3] Implement mark_rejected() — fire block_rejected, remove block + descendants, handle chain switch in modules/consensus/src/consensus_tree.rs
- [x] T042 [US3] Implement remove_block() — remove block + descendants, detect chain switch, return newly wanted hashes in modules/consensus/src/consensus_tree.rs (FR-010)

**Checkpoint**: All P1 user stories complete. Core consensus tree fully functional.

---

## Phase 6: User Story 4 - Prune Immutable Blocks (Priority: P2)

**Goal**: Discard blocks deeper than k from tip, remove dead forks,
bound memory usage.

**Independent Test**: Add k+1 blocks, verify old blocks removed.
Add fork before prune boundary, verify non-favoured branch pruned.

### Tests for User Story 4

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [x] T043 [P] [US4] Test prune removes blocks with number < (tip - k) from tree and hash map in modules/consensus/src/consensus_tree.rs (SC-004)
- [x] T044 [P] [US4] Test prune removes non-favoured branch rooted before prune boundary in modules/consensus/src/consensus_tree.rs (SC-004)
- [x] T045 [P] [US4] Test prune preserves both branches of fork after prune boundary in modules/consensus/src/consensus_tree.rs
- [x] T046 [P] [US4] Test prune updates root to new oldest block in modules/consensus/src/consensus_tree.rs

### Implementation for User Story 4

- [x] T047 [US4] Implement prune() — remove blocks older than (tip - k), use chain_contains to identify and remove dead forks, update root in modules/consensus/src/consensus_tree.rs (FR-011)

**Checkpoint**: All user stories independently functional.

---

## Phase 7: Integration with Consensus Module

**Purpose**: Wire the tree into the existing consensus module's bus subscriptions

- [x] T048 Implement ConsensusTreeObserver for consensus module — block_proposed publishes to cardano.block.proposed, rollback publishes state transition, block_rejected publishes to cardano.block.rejected in modules/consensus/src/consensus.rs
- [x] T049 Rewire consensus.rs to create ConsensusTree on init, subscribe to cardano.block.offered and cardano.block.rescinded, route messages to tree operations in modules/consensus/src/consensus.rs
- [x] T050 Wire validation responses (cardano.validation.*) back to tree via mark_validated/mark_rejected in modules/consensus/src/consensus.rs
- [x] T051 Add security-parameter config key (default 2160) to consensus module configuration in modules/consensus/src/consensus.rs and processes/omnibus/omnibus.toml

**Checkpoint**: Consensus module uses tree for chain selection. `// TODO` removed.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Final quality checks and documentation

- [x] T052 Run `make fmt && make clippy && make test` — fix any warnings or failures
- [x] T053 [P] Verify all public types/functions have `///` doc comments per constitution Principle I in modules/consensus/src/
- [x] T054 [P] Run quickstart.md validation — verify build/test commands work as documented in specs/1-consensus-tree/quickstart.md

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Foundational — provides base tree operations
- **US2 (Phase 4)**: Depends on US1 (needs check_block_wanted for add_block context)
- **US3 (Phase 5)**: Depends on US1 + US2 (needs both for rollback/validation)
- **US4 (Phase 6)**: Depends on Foundational only — can run parallel with US2/US3
- **Integration (Phase 7)**: Depends on US1 + US2 + US3 + US4
- **Polish (Phase 8)**: Depends on all previous phases

### Parallel Opportunities

- **Phase 1**: T002, T003, T004 can all run in parallel (different files)
- **Phase 2**: All test tasks (T008-T013) in parallel; T016, T017 in parallel
- **Phase 3**: All test tasks (T018-T022) in parallel
- **Phase 4**: All test tasks (T024-T029) in parallel
- **Phase 5**: All test tasks (T031-T038) in parallel; T040, T041 in parallel
- **Phase 6**: All test tasks (T043-T046) in parallel
- **US4 can run in parallel with US2/US3** (no dependency between them)

### Within Each User Story

- Tests MUST be written and FAIL before implementation
- Implementation tasks depend on their tests existing
- Story complete = all tests pass

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3: User Story 1
4. **STOP and VALIDATE**: Tree correctly selects favoured chain for 10+ topologies
5. Can be reviewed/merged as standalone increment

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. Add US1 → Test independently → core tree works (MVP)
3. Add US2 → Test independently → block ingestion works
4. Add US3 → Test independently → rollbacks + validation loop
5. Add US4 → Test independently → memory bounded
6. Integration → wire to bus → replaces TODO in consensus.rs
7. Polish → quality gates pass

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable
- Verify tests fail before implementing
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
- All tests live in `#[cfg(test)] mod tests` within consensus_tree.rs
