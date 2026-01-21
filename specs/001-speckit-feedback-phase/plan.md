# Implementation Plan: SpecKit Feedback Phase

**Branch**: `001-speckit-feedback-phase` | **Date**: 2026-01-20 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/001-speckit-feedback-phase/spec.md`

## Summary

Add a `/speckit.feedback` command to the local speckit tooling that extracts lessons learned from PR review comments and discussions, stores them in per-PR lesson files and a central lessons database, enabling future speckit phases to leverage accumulated organizational knowledge.

## Technical Context

**Language/Version**: Markdown agent files (VS Code Chat Agents), Bash scripts for automation  
**Primary Dependencies**: GitHub CLI (`gh`) for PR data retrieval, VS Code Chat Participant API  
**Storage**: Markdown files with YAML frontmatter (`docs/feedback/lessons.md`, `docs/feedback/pr-<number>-lessons.md`)  
**Testing**: Manual testing via VS Code chat invocation; script validation via bash  
**Target Platform**: VS Code with GitHub Copilot extension  
**Project Type**: Single (agent + prompt + supporting scripts)  
**Performance Goals**: < 2 minutes for full feedback extraction workflow  
**Constraints**: GitHub-only (no GitLab/Bitbucket), English PR comments only  
**Scale/Scope**: Lessons database grows indefinitely; target 100+ lessons with deduplication

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

The constitution template is not yet customized for this project. Proceeding with general best practices:

| Principle | Status | Notes |
|-----------|--------|-------|
| File-based artifacts | ✅ Pass | Lessons stored in markdown files |
| Minimal dependencies | ✅ Pass | Only requires `gh` CLI (standard GitHub tooling) |
| Testable outputs | ✅ Pass | Generated files can be validated |
| Simplicity | ✅ Pass | Follows existing speckit agent patterns |

## Project Structure

### Documentation (this feature)

```text
specs/001-speckit-feedback-phase/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output (N/A for this feature - no API)
└── tasks.md             # Phase 2 output (/speckit.tasks command)
```

### Source Code (repository root)

```text
.github/
├── agents/
│   ├── speckit.feedback.agent.md    # NEW: Feedback agent definition
│   ├── speckit.specify.agent.md     # MODIFY: Add lessons database integration
│   ├── speckit.plan.agent.md        # MODIFY: Add lessons database integration
│   └── speckit.implement.agent.md   # MODIFY: Add lessons database integration
└── prompts/
    └── speckit.feedback.prompt.md   # NEW: Feedback prompt registration

.specify/
└── scripts/
    └── bash/
        └── fetch-pr-feedback.sh     # NEW: Helper script for PR data extraction

docs/
└── feedback/
    ├── lessons.md                   # NEW: Central lessons database
    └── pr-<number>-lessons.md       # NEW: Per-PR lessons files (generated)
```

**Structure Decision**: Follows existing speckit agent pattern (agent.md + prompt.md pair). Helper script added to `.specify/scripts/bash/` for reusable PR data fetching. Lessons stored in `docs/feedback/` for discoverability. Existing agents (specify, plan, implement) modified to read lessons database.

## Complexity Tracking

> No constitution violations requiring justification.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| N/A | N/A | N/A |

---

## Files to Create

| File | Purpose |
|------|---------|
| `.github/agents/speckit.feedback.agent.md` | Agent definition with execution workflow |
| `.github/prompts/speckit.feedback.prompt.md` | Prompt registration (minimal, points to agent) |
| `.specify/scripts/bash/fetch-pr-feedback.sh` | Helper script to fetch PR data via `gh` CLI |
| `docs/feedback/lessons.md` | Initial empty lessons database with header template |

## Files to Modify

| File | Modification |
|------|--------------|
| `.github/agents/speckit.specify.agent.md` | Add step to read `docs/feedback/lessons.md` and incorporate relevant lessons when generating specifications (avoid past specification mistakes) |
| `.github/agents/speckit.plan.agent.md` | Add step to read `docs/feedback/lessons.md` and incorporate relevant lessons when creating implementation plans (apply known patterns/anti-patterns) |
| `.github/agents/speckit.implement.agent.md` | Add step to read `docs/feedback/lessons.md` and incorporate relevant lessons when writing code (follow established code quality lessons) |

### Integration Pattern for Existing Agents

Each modified agent should add the following step early in its execution flow:

```markdown
## Lessons Integration

1. Check if `docs/feedback/lessons.md` exists
2. If exists, read and parse the lessons database
3. Filter lessons relevant to the current task:
   - For specify: filter by categories [architecture, documentation, other]
   - For plan: filter by categories [architecture, testing, performance]
   - For implement: filter by categories [code-quality, security, testing, performance]
4. Include filtered lessons as context for the agent's decision-making
5. Reference applied lessons in output where appropriate
```
