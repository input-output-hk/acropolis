# Feature Specification: SpecKit Feedback Phase

**Feature Branch**: `001-speckit-feedback-phase`  
**Created**: 2026-01-20  
**Status**: Draft  
**Input**: User description: "Add a /speckit.feedback phase to the local speckit tooling"

## Clarifications

### Session 2026-01-20

- Q: Should lessons files be named per-branch or per-PR? → A: Per-PR (using PR number)
- Q: How should lessons be surfaced to future speckit phases? → A: Agent instructions explicitly read lessons file as context
- Q: What category taxonomy for lessons? → A: Fixed categories + optional free-form tags
- Q: How to determine which PR to extract feedback from? → A: Support --pr flag for explicit selection, default to most recently merged PR for current branch
- Q: What format for lessons database? → A: Markdown with YAML frontmatter per lesson block
- Q: What sources to extract feedback from? → A: Review comments + PR description (not commit messages)

### Session 2026-01-21

- Q: Should agent modification be an explicit functional requirement? → A: Yes, add FR-011 for existing phase integration
- Q: Can FR-011 be implemented without modifying existing agents? → A: Yes, use AGENTS.md + CLAUDE.md in .specify/memory/feedback/ for cross-platform compatibility (Copilot + Claude Code)

### Session 2026-01-22

- Q: Can `/speckit.feedback` be run multiple times on the same PR before merge? → A: Yes, subsequent runs merge new lessons with existing ones (incremental updates)
- Q: What if new comments are added after the first feedback run? → A: Running feedback again extracts only new comments and merges them into existing lesson files
- Q: Can manual lessons be added between feedback runs? → A: Yes, manual lessons are preserved and merged with PR-extracted lessons
- Q: Should we automate the git add/commit/push workflow? → A: Yes, add optional `--commit` flag to run git commands automatically

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Capture PR Feedback Before Merge (Priority: P1)

As a developer whose PR has received reviews, I want to run `/speckit.feedback` to automatically extract lessons learned from PR review comments and discussions before merging, so that these insights are committed as part of the original PR and preserved for future development cycles.

**Why this priority**: This is the core value proposition - without capturing feedback, the entire feature has no purpose. Running feedback capture before merge simplifies the commit workflow since lessons can be added directly to the feature branch.

**Independent Test**: Can be fully tested by running `/speckit.feedback` on an open PR with reviews and verifying that a lessons document is created with extracted insights, then committing those files as part of the PR.

**Acceptance Scenarios**:

1. **Given** a PR is open with completed reviews on the current branch, **When** I run `/speckit.feedback`, **Then** the agent identifies the associated PR and extracts review comments, suggestions, and discussions.

2. **Given** PR review comments contain actionable feedback (e.g., "consider using expect() instead of unwrap()"), **When** the agent processes the feedback, **Then** it categorizes and summarizes each piece of feedback into a structured format.

3. **Given** the feedback extraction completes, **When** the agent writes the lessons file, **Then** a new file is created at `.specify/memory/feedback/pr-<pr-number>-lessons.md` containing the structured feedback, ready to be committed as part of the current branch.

4. **Given** lessons have been extracted to files, **When** I commit and merge the PR, **Then** the lessons are persisted to main as part of the original PR without requiring a separate commit workflow.

5. **Given** I have already run `/speckit.feedback` on a PR, **When** new review comments are added and I run `/speckit.feedback` again, **Then** the agent extracts only the new feedback and merges it with the existing lessons (no duplicates, incremental update).

6. **Given** I have run `/speckit.feedback` on a PR, **When** I add a manual lesson before merging, **Then** the manual lesson is preserved alongside the PR-extracted lessons.

---

### User Story 2 - Update Compounding Lessons Database (Priority: P2)

As a developer, I want the feedback phase to update a central lessons database, so that accumulated knowledge from all PRs is available to inform future speckit phases (specify, plan, implement).

**Why this priority**: This enables the compounding value of the system - individual lessons become organizational knowledge that improves all future work.

**Independent Test**: Can be tested by running `/speckit.feedback` on multiple PRs and verifying that `.specify/memory/lessons.md` grows with each run, maintaining proper categorization.

**Acceptance Scenarios**:

1. **Given** new lessons have been extracted from a PR, **When** the agent updates the lessons database, **Then** the lessons are appended to `.specify/memory/lessons.md` with proper categorization (code quality, architecture, testing, documentation, etc.).

2. **Given** similar feedback has been received before (e.g., multiple "use expect() not unwrap()" comments), **When** the agent processes duplicate patterns, **Then** it consolidates them into a single lesson with increased frequency count rather than duplicating entries.

3. **Given** lessons exist in the database, **When** future speckit phases run (specify, plan, implement), **Then** those phases can reference the lessons to avoid repeating past mistakes.

---

### User Story 3 - Manual Feedback Entry (Priority: P3)

As a developer, I want to manually add lessons learned even without a PR context, so that insights from pair programming, architecture discussions, or external code reviews can be captured.

**Why this priority**: Extends the utility beyond just PR-based feedback, but is not essential for the core workflow.

**Independent Test**: Can be tested by running `/speckit.feedback "Always validate user input before database queries"` and verifying the lesson is added to the database.

**Acceptance Scenarios**:

1. **Given** I provide a lesson as an argument (e.g., `/speckit.feedback "Use structured logging for production code"`), **When** the command executes, **Then** the lesson is added to `.specify/memory/lessons.md` with a "manual" source tag.

2. **Given** I provide a category hint (e.g., `/speckit.feedback --category security "Sanitize all user inputs"`), **When** the command executes, **Then** the lesson is categorized accordingly.

---

### Edge Cases

- What happens when the current branch has no associated PR? → Agent prompts for manual feedback entry or searches for open/recently merged PRs.
- What happens when the PR has no review comments? → Agent reports "No feedback found" and offers manual entry option.
- What happens when `.specify/memory/feedback/` directory doesn't exist? → Agent creates the directory structure automatically.
- How does system handle very long PR discussions? → Agent summarizes and prioritizes the most actionable items, limiting to top 10 lessons per PR.
- What happens when running feedback on the same PR twice? → Agent merges new lessons with existing ones, incrementing frequency for duplicates and adding only new lessons (no overwrite prompt needed).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST provide a `/speckit.feedback` command accessible via VS Code chat interface.
- **FR-002**: System MUST support explicit PR selection via `--pr <number>` argument; if not provided, system MUST default to the open PR associated with the current branch (or most recently merged if no open PR exists).
- **FR-003**: System MUST extract feedback from review comments, review suggestions, discussion threads, and the PR description.
- **FR-004**: System MUST categorize extracted feedback into predefined categories (code-quality, architecture, testing, documentation, security, performance, other).
- **FR-005**: System MUST generate a structured lessons document at `.specify/memory/feedback/pr-<pr-number>-lessons.md`.
- **FR-006**: System MUST update the central lessons database at `.specify/memory/lessons.md` with new insights.
- **FR-007**: System MUST detect and consolidate duplicate or similar lessons rather than creating redundant entries.
- **FR-008**: System MUST support manual lesson entry via command arguments when no PR context exists.
- **FR-009**: System MUST preserve existing lessons when updating the database (append-only, no destructive updates).
- **FR-010**: System MUST report a summary of captured lessons to the user upon completion.
- **FR-011**: Existing speckit phases (specify, plan, implement) MUST be able to read `.specify/memory/lessons.md` and incorporate relevant lessons as context when generating their outputs. This MUST be achieved via agent instruction files (`AGENTS.md` and `CLAUDE.md`) co-located with the lessons database, NOT by modifying existing agent files.
- **FR-012**: System MUST support incremental updates when run multiple times on the same PR, merging new lessons with existing ones rather than overwriting.
- **FR-013**: System MUST support a `--commit` flag that automatically runs `git add .specify/memory/feedback/`, `git commit -m "chore(feedback): capture lessons from PR #<number>"`, and `git push` after extracting lessons.

### Key Entities

- **Lesson**: A discrete piece of feedback or learning. Attributes: content, category, tags (optional free-form), source (PR/manual), date, frequency count.
- **Lesson Category**: A fixed classification for organizing lessons (code-quality, architecture, testing, documentation, security, performance, other). Lessons may also have optional free-form tags for additional context (e.g., `rust`, `async`, `error-handling`).
- **Lessons Database**: The central file (`.specify/memory/lessons.md`) containing all accumulated lessons. Format: Markdown with YAML frontmatter per lesson block containing metadata (category, tags, source, date, frequency count).
- **PR Lessons File**: A per-PR file (`.specify/memory/feedback/pr-<pr-number>-lessons.md`) containing lessons from a specific PR.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Developer can capture PR feedback and generate lessons file in under 2 minutes from command invocation.
- **SC-002**: 80% of actionable PR review comments are successfully extracted and categorized by the system.
- **SC-003**: Lessons database remains consistent and searchable as it grows beyond 100 entries.
- **SC-004**: Future speckit phases (specify, plan, implement) can query the lessons database to inform their outputs.
- **SC-005**: Duplicate lessons are consolidated with 95% accuracy (no more than 5% redundant entries).

## Assumptions

- The project uses GitHub for PR management and the GitHub API (or gh CLI) is available for querying PR data.
- Developers run `/speckit.feedback` from within VS Code with GitHub Copilot chat available.
- The git repository has a remote configured and branch names can be correlated to PRs.
- PR review comments are written in English.

## Out of Scope

- Integration with non-GitHub platforms (GitLab, Bitbucket) - future enhancement.
- Automatic feedback without user invocation (no pre-merge hooks or CI integration).
- Sentiment analysis or prioritization of feedback by importance.
- Cross-repository lesson aggregation.

## Dependencies

- Existing speckit infrastructure (`.specify/` directory structure, agent patterns).
- GitHub API or gh CLI for PR data retrieval.
- VS Code chat participant API for command registration.

---

*End of Specification*
