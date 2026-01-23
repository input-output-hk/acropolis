# Tasks: SpecKit Feedback Phase

**Input**: Design documents from `/specs/001-speckit-feedback-phase/`
**Prerequisites**: plan.md ✓, spec.md ✓, research.md ✓, data-model.md ✓, quickstart.md ✓

**Tests**: Not explicitly requested in specification - omitting test tasks.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and directory structure

- [X] T001 Create `docs/feedback/` directory structure
- [X] T002 [P] Create initial lessons database template at `docs/feedback/lessons.md` with YAML frontmatter header and category documentation
- [X] T003 [P] Create prompt registration file at `.github/prompts/speckit.feedback.prompt.md`
- [X] T004 [P] Create agent instruction file at `docs/feedback/AGENTS.md` for GitHub Copilot integration
- [X] T005 [P] Create agent instruction file at `docs/feedback/CLAUDE.md` for Claude Code integration (identical content to AGENTS.md)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T006 Create `fetch-pr-feedback.sh` helper script at `.specify/scripts/bash/fetch-pr-feedback.sh` with PR data extraction via `gh` CLI
- [X] T007 Create feedback agent skeleton at `.github/agents/speckit.feedback.agent.md` with YAML frontmatter, $ARGUMENTS block, and outline structure

**Checkpoint**: Foundation ready - user story implementation can now begin

---

## Phase 3: User Story 1 - Capture PR Feedback Before Merge (Priority: P1) 🎯 MVP

**Goal**: Extract lessons from PR review comments and create per-PR lessons file

**Independent Test**: Run `/speckit.feedback --pr <number>` and verify `docs/feedback/pr-<number>-lessons.md` is created with structured lessons

### Implementation for User Story 1

- [X] T008 [US1] Implement argument parsing in agent for `--pr <number>` flag in `.github/agents/speckit.feedback.agent.md`
- [X] T009 [US1] Implement PR detection logic: if no `--pr` flag, find most recently merged PR for current branch via `gh pr list --state merged`
- [X] T010 [US1] Implement PR data extraction step using `fetch-pr-feedback.sh` to get review comments, suggestions, and PR description
- [X] T011 [US1] Implement lesson categorization logic using LLM to classify feedback into categories (code-quality, architecture, testing, documentation, security, performance, other)
- [X] T012 [US1] Implement PR lessons file generation at `docs/feedback/pr-<pr-number>-lessons.md` with YAML frontmatter per data-model.md schema
- [X] T013 [US1] Implement user summary output showing count of lessons by category

**Checkpoint**: User Story 1 complete - can extract PR feedback and create per-PR lessons file

---

## Phase 4: User Story 2 - Update Compounding Lessons Database (Priority: P2)

**Goal**: Append lessons to central database with deduplication

**Independent Test**: Run `/speckit.feedback` on multiple PRs and verify `docs/feedback/lessons.md` grows with consolidated lessons

### Implementation for User Story 2

- [X] T014 [US2] Implement lessons database reading and parsing in `.github/agents/speckit.feedback.agent.md`
- [X] T015 [US2] Implement duplicate detection logic (fuzzy match on lesson content) to find similar existing lessons
- [X] T016 [US2] Implement frequency increment for duplicate lessons instead of creating new entries
- [X] T017 [US2] Implement new lesson appending with unique lesson_id generation (L001, L002, etc.)
- [X] T018 [US2] Implement database metadata update (last_updated, total_lessons count)
- [X] T019 [US2] Implement incremental merge: if `docs/feedback/pr-<number>-lessons.md` exists, merge new lessons with existing ones (no overwrite prompt, incremental update per FR-012)

**Checkpoint**: User Stories 1 AND 2 complete - full PR feedback workflow operational

---

## Phase 5: User Story 3 - Manual Feedback Entry (Priority: P3)

**Goal**: Allow manual lesson entry without PR context

**Independent Test**: Run `/speckit.feedback "lesson text"` and verify lesson is added to `docs/feedback/lessons.md` with "manual" source

### Implementation for User Story 3

- [X] T020 [US3] Implement argument parsing for inline lesson text (non-flag argument) in `.github/agents/speckit.feedback.agent.md`
- [X] T021 [US3] Implement `--category <category>` flag parsing for manual category assignment
- [X] T022 [US3] Implement manual lesson flow: skip PR extraction, go directly to database update with source="manual"
- [X] T023 [US3] Implement interactive category selection when no `--category` flag provided (prompt user)

**Checkpoint**: All user stories for feedback agent complete

---

## ~~Phase 6: Existing Agent Integration (FR-011)~~ — REMOVED

**Note**: FR-011 is now satisfied by T004 and T005 in Phase 1 (creating `AGENTS.md` and `CLAUDE.md` in `docs/feedback/`). No modifications to existing agent files are required.

The co-located instruction files are automatically discovered by:
- **GitHub Copilot**: finds "nearest AGENTS.md in directory tree"
- **Claude Code**: reads "CLAUDE.md from child directories"

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Documentation and edge case handling

- [X] T024 Implement edge case: no associated PR found → prompt for manual entry or search recently merged PRs
- [X] T025 Implement edge case: PR has no review comments → report "No feedback found" and offer manual entry
- [X] T026 Implement edge case: `docs/feedback/` directory doesn't exist → create automatically
- [X] T027 Implement edge case: very long PR discussions → summarize and limit to top 10 lessons
- [X] T028 [P] Update `specs/001-speckit-feedback-phase/quickstart.md` with final usage examples after implementation
- [X] T029 Run quickstart.md validation to verify all documented commands work

---

## Dependencies & Execution Order

### Phase Dependencies

```
Phase 1: Setup ──────────────────────────────────┐
                                                 │
Phase 2: Foundational ───────────────────────────┤
                                                 │
         ┌───────────────────────────────────────┘
         │
         ▼
Phase 3: User Story 1 (P1) ──────┐
         │                       │
         ▼                       │ (can parallelize if staffed)
Phase 4: User Story 2 (P2) ──────┤
         │                       │
         ▼                       │
Phase 5: User Story 3 (P3) ──────┘
         │
         ▼
Phase 6: Polish
```

### User Story Dependencies

- **User Story 1 (P1)**: Depends on Phase 2 completion. No dependencies on other stories.
- **User Story 2 (P2)**: Depends on Phase 2 completion. Builds on US1 output (lessons to add to database).
- **User Story 3 (P3)**: Depends on Phase 2 completion. Independent of US1/US2 (manual entry path).
- **Agent Integration**: Now handled in Phase 1 Setup (T004, T005) - no separate phase needed.

### Parallel Opportunities

**Within Setup (Phase 1)**:

**Cross-Phase**:

**Cross-Phase**:
US3 (T020-T023) can start immediately after Phase 2, in parallel with US1/US2 work

---

## Implementation Strategy

### MVP Scope (Recommended First Delivery)

**Phases 1-3 only** = User Story 1 (Capture PR Feedback)

This delivers:
- Working `/speckit.feedback --pr <number>` command
- Per-PR lessons file generation
- Categorized lessons output
- AGENTS.md and CLAUDE.md for cross-platform lesson surfacing

Value: Immediately usable for capturing PR feedback, even without database consolidation.

### Incremental Delivery

1. **MVP**: Phases 1-3 (US1) - Basic PR feedback capture + agent integration
2. **+Database**: Phase 4 (US2) - Consolidated lessons with deduplication
3. **+Manual**: Phase 5 (US3) - Manual entry support
4. **+Polish**: Phase 6 - Edge cases and documentation

---

## Summary

| Metric | Count |
|--------|-------|
| **Total Tasks** | 29 |
| **Setup Tasks** | 5 |
| **Foundational Tasks** | 2 |
| **US1 Tasks** | 6 |
| **US2 Tasks** | 6 |
| **US3 Tasks** | 4 |
| **Polish Tasks** | 6 |
| **Parallelizable Tasks** | 6 |

| User Story | Task Range | Parallel Opportunities |
|------------|------------|------------------------|
| Setup | T001-T005 | T002+T003, T004+T005 |
| Foundational | T006-T007 | None (sequential) |
| US1 (P1) | T008-T013 | None (sequential flow) |
| US2 (P2) | T014-T019 | None (sequential flow) |
| US3 (P3) | T020-T023 | Can run parallel to US1/US2 |
| Polish | T024-T029 | T028 |
