# Tasks: Plutus Phase 2 Script Validation

**Input**: Design documents from `/specs/568-plutus-phase2-validation/`  
**Prerequisites**: plan.md ✓, spec.md ✓, research.md ✓, data-model.md ✓, contracts/ ✓

## Format: `[ID] [P?] [Story?] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- All paths are relative to repository root

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and dependency configuration

- [X] T001 Add `uplc-turbo` dependency to workspace Cargo.toml under `[workspace.dependencies]`
- [X] T002 Add `uplc-turbo = { workspace = true }` and `rayon = "1.10"` to modules/tx_unpacker/Cargo.toml
- [X] T003 [P] Create validations/phase2.rs module file with stub exports in modules/tx_unpacker/src/validations/phase2.rs
- [X] T004 [P] Add `pub mod phase2;` to modules/tx_unpacker/src/validations/mod.rs
- [X] T005 [P] Create phase2_test.rs integration test file with inline test script constants in modules/tx_unpacker/tests/phase2_test.rs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types and error handling that ALL user stories depend on

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T006 Define `ExBudget` struct in modules/tx_unpacker/src/validations/phase2.rs
- [X] T007 Define `Phase2Error` enum with thiserror derives in modules/tx_unpacker/src/validations/phase2.rs
- [X] T008 Define `ScriptPurpose` enum for spending/minting/certifying/rewarding/voting/proposing in modules/tx_unpacker/src/validations/phase2.rs
- [X] T009 Add `phase2_enabled: bool` field to tx_unpacker module configuration struct
- [X] T010 Add `Phase2(Phase2Error)` variant to `ValidationError` enum in common/src/validation.rs

**Checkpoint**: Foundation ready - user story implementation can now begin

---

## Phase 3: User Story 1 - Single Script Validation (Priority: P1) 🎯 MVP

**Goal**: Evaluate individual Plutus scripts within transactions using uplc-turbo crate

**Independent Test**: Submit a block with a single Plutus script transaction and verify validation result

### Implementation for User Story 1

- [X] T011 [US1] Write `test_eval_always_succeeds` in modules/tx_unpacker/tests/phase2_test.rs (TDD: expect RED)
- [X] T012 [US1] Implement `evaluate_script()` function with basic uplc integration in modules/tx_unpacker/src/validations/phase2.rs
- [X] T013 [US1] Write `test_eval_always_fails` in modules/tx_unpacker/tests/phase2_test.rs (TDD: expect RED)
- [X] T014 [US1] Handle `MachineError::ExplicitErrorTerm` returning `Phase2Error::ScriptFailed` in modules/tx_unpacker/src/validations/phase2.rs
- [X] T015 [US1] Write `test_eval_budget_exceeded` in modules/tx_unpacker/tests/phase2_test.rs (TDD: expect RED)
- [X] T016 [US1] Handle `MachineError::OutOfExError` returning `Phase2Error::BudgetExceeded` in modules/tx_unpacker/src/validations/phase2.rs
- [X] T017 [US1] Write `test_eval_spending_validator` with 3 args (datum, redeemer, context) in modules/tx_unpacker/tests/phase2_test.rs (TDD: expect RED)
- [X] T018 [US1] Implement argument application for spending validators in modules/tx_unpacker/src/validations/phase2.rs
- [X] T019 [US1] Write `test_eval_minting_policy` with 2 args (redeemer, context) in modules/tx_unpacker/tests/phase2_test.rs (TDD: expect RED)
- [X] T020 [US1] Implement argument application for minting policies in modules/tx_unpacker/src/validations/phase2.rs
- [X] T021 [US1] Implement `build_script_context()` helper to construct PlutusData ScriptContext in modules/tx_unpacker/src/validations/phase2.rs
- [X] T022 [US1] Write `test_eval_plutus_v1_script` verifying V1 cost model in modules/tx_unpacker/tests/phase2_test.rs
- [X] T023 [US1] Write `test_eval_plutus_v2_script` verifying V2 cost model and reference inputs in modules/tx_unpacker/tests/phase2_test.rs
- [X] T024 [US1] Write `test_eval_plutus_v3_script` verifying V3 cost model and governance context in modules/tx_unpacker/tests/phase2_test.rs
- [X] T025 [US1] Add doc comments to all public functions and types in modules/tx_unpacker/src/validations/phase2.rs

### Integration for User Story 1 (Plan Phase 3: Integration)

- [X] T026 [US1] Implement `validate_transaction_phase2()` to extract and match scripts with redeemers in modules/tx_unpacker/src/validations/phase2.rs
- [X] T027 [US1] Implement script-to-redeemer matching logic using ScriptPurpose in modules/tx_unpacker/src/validations/phase2.rs
- [X] T028 [US1] Wire `validate_transaction_phase2()` into validation flow in modules/tx_unpacker/src/state.rs (FR-002: after Phase 1 passes)
- [X] T029 [US1] Write `test_phase2_enabled_validates_scripts` integration test in modules/tx_unpacker/tests/phase2_test.rs
- [X] T030 [US1] Ensure Phase 2 errors are reported via ValidationError::Phase2 variant (FR-010) in modules/tx_unpacker/src/state.rs

**Checkpoint**: Single script evaluation works end-to-end - can validate any block with Plutus V1/V2/V3 scripts (FR-003, FR-010 verified) ✓

---

## Phase 4: User Story 2 - Multi-Script Block Validation (Priority: P2)

**Goal**: Efficiently validate multiple scripts in a block using parallel execution

**Independent Test**: Submit block with 10 scripts, verify total time < sequential baseline

### Implementation for User Story 2

- [ ] T031 [US2] Write `test_parallel_multi_script_block` in modules/tx_unpacker/tests/phase2_test.rs (TDD: expect RED)
- [ ] T032 [US2] Implement parallel evaluation using `rayon::par_iter()` in modules/tx_unpacker/src/validations/phase2.rs
- [ ] T033 [US2] Ensure arena allocation is per-thread for thread safety (FR-009: constant memory) in modules/tx_unpacker/src/validations/phase2.rs
- [ ] T034 [US2] Handle early-exit on first script failure with proper error aggregation in modules/tx_unpacker/src/validations/phase2.rs

**Checkpoint**: Multi-script blocks validate efficiently in parallel

---

## Phase 5: User Story 3 - Configuration-Gated Validation (Priority: P3)

**Goal**: Enable/disable Phase 2 validation via configuration flag (default: disabled)

**Independent Test**: Toggle config flag and observe validation behavior change

### Implementation for User Story 3

- [ ] T035 [US3] Write `test_phase2_disabled_skips_scripts` in modules/tx_unpacker/tests/phase2_test.rs (TDD: expect RED)
- [ ] T036 [US3] Add config flag check in `state.rs::validate()` to conditionally call Phase 2 in modules/tx_unpacker/src/state.rs
- [ ] T037 [US3] Refactor state.rs to respect phase2_enabled configuration flag (default: disabled)
- [ ] T038 [US3] Add configuration documentation to omnibus.toml template or README

**Checkpoint**: Phase 2 validation can be toggled on/off via config

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, edge cases, final validation, and benchmark preparation

- [ ] T039 [P] Add integration test with real Conway mainnet transaction fixture in modules/tx_unpacker/tests/phase2_test.rs
- [ ] T040 [P] Handle malformed script bytes gracefully returning `Phase2Error::DecodeFailed` in modules/tx_unpacker/src/validations/phase2.rs
- [ ] T041 [P] Handle missing datum with `Phase2Error::MissingDatum` in modules/tx_unpacker/src/validations/phase2.rs
- [ ] T042 [P] Handle missing redeemer with `Phase2Error::MissingRedeemer` in modules/tx_unpacker/src/validations/phase2.rs
- [ ] T043 Create initial "Plutus Phase 2 Golden Corpus v1" test fixtures from mainnet samples in tests/fixtures/phase2_corpus/ (for SC-001..SC-005)
- [ ] T044 Run `cargo test -p acropolis_module_tx_unpacker phase2` and verify all tests pass
- [ ] T045 Run `cargo clippy -p acropolis_module_tx_unpacker` and fix any warnings
- [ ] T046 Validate quickstart.md examples compile and work

---

## Dependencies & Execution Order

### Phase Dependencies

```
Phase 1: Setup ──────────────────┐
                                 │
Phase 2: Foundational ───────────┤ (BLOCKS all user stories)
                                 │
                                 ▼
Phase 3: US1 (P1) ───────────────┤ (Core + Integration = MVP)
Single Script + Integration      │
                                 │
         ┌───────────────────────┴───────────────────────┐
         │                                               │
         ▼                                               ▼
Phase 4: US2 (P2)                                 Phase 5: US3 (P3)
Multi-Script Parallel                             Configuration Toggle
         │                                               │
         └───────────────────────┬───────────────────────┘
                                 │
                                 ▼
                    Phase 6: Polish & Cross-Cutting
```

### User Story Dependencies

- **User Story 1 (P1)**: Requires Foundational phase only - includes integration into state.rs for MVP
- **User Story 2 (P2)**: Requires US1 `validate_transaction_phase2()` function - adds parallel execution
- **User Story 3 (P3)**: Requires US1 integration - adds config toggle to disable feature

### Within Each User Story (TDD Order)

1. Write test (RED)
2. Implement code (GREEN)
3. Refactor if needed
4. Next test

### Parallel Opportunities

**Setup Phase (T001-T005)**:
- T003, T004, T005 can run in parallel (different files)

**Foundational Phase (T006-T010)**:
- T006, T007, T008 can run in parallel (same file but independent definitions)

**Polish Phase (T039-T046)**:
- T039, T040, T041, T042 can run in parallel (different concerns)

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational
3. Complete Phase 3: User Story 1 (includes integration into state.rs)
4. **STOP and VALIDATE**: Can validate blocks with single Plutus scripts end-to-end
5. Ship MVP if needed

### Incremental Delivery

| Milestone | Delivers | Independently Testable |
|-----------|----------|------------------------|
| US1 Complete | Single script validation wired into system | ✅ Yes - blocks process through state.rs |
| US2 Complete | Parallel multi-script blocks | ✅ Yes |
| US3 Complete | Config-gated feature toggle | ✅ Yes |
| Polish Complete | Production-ready | ✅ Yes |

### Estimated Task Count by Story

| Phase | Tasks | Parallelizable |
|-------|-------|----------------|
| Setup | 5 | 3 |
| Foundational | 5 | 0 |
| US1 (P1) | 20 | 0 (TDD sequence) |
| US2 (P2) | 4 | 0 (TDD sequence) |
| US3 (P3) | 4 | 0 (TDD sequence) |
| Polish | 8 | 4 |
| **Total** | **46** | **7** |

---

## Notes

- All tests follow TDD: write test first, observe RED, then implement
- `evaluate_script()` is the core function - everything builds on it
- Use `rayon::par_iter()` for parallel execution (not tokio - CPU-bound work)
- Arena allocator ensures constant memory per script evaluation
- Phase 2 only runs if Phase 1 passes (fail-fast)
- Configuration defaults to `phase2_enabled = false` for safe rollout
