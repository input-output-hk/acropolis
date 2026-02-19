# Implementation Plan: SpecKit Feedback Phase

**Branch**: `001-speckit-feedback-phase` | **Date**: 2026-01-20 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/001-speckit-feedback-phase/spec.md`

## Summary

Add a `/speckit.feedback` command to the local speckit tooling that extracts lessons learned from PR review comments and discussions before merge, stores them in per-PR lesson files and a central lessons database. By running before merge, lessons can be committed directly to the feature branch and merged as part of the original PR, simplifying persistence while enabling future speckit phases to leverage accumulated organizational knowledge. The command supports incremental updates - running multiple times on the same PR merges new lessons with existing ones.

## Technical Context

**Language/Version**: Markdown agent files (VS Code Chat Agents), Bash scripts for automation  
**Primary Dependencies**: GitHub CLI (`gh`) for PR data retrieval, VS Code Chat Participant API  
**Storage**: Markdown files with YAML frontmatter (`.specify/memory/lessons.md`, `.specify/memory/feedback/pr-<number>-lessons.md`)  
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
│   └── speckit.feedback.agent.md    # NEW: Feedback agent definition
└── prompts/
    └── speckit.feedback.prompt.md   # NEW: Feedback prompt registration

.specify/
├── scripts/
│   └── bash/
│       └── fetch-pr-feedback.sh     # NEW: Helper script for PR data extraction
└── memory/
    ├── lessons.md                   # NEW: Central lessons database
    └── feedback/
        └── pr-<number>-lessons.md   # NEW: Per-PR lessons files (generated)
```

**Structure Decision**: Follows existing speckit agent pattern (agent.md + prompt.md pair). Helper script added to `.specify/scripts/bash/` for reusable PR data fetching. Lessons stored in `.specify/memory/feedback/` for discoverability.

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
| `.specify/memory/lessons.md` | Initial empty lessons database with header template |
| `.specify/memory/feedback/AGENTS.md` | Agent instructions for GitHub Copilot to read lessons database |
| `.specify/memory/feedback/CLAUDE.md` | Agent instructions for Claude Code to read lessons database (identical to AGENTS.md) |

## Cross-Platform Agent Integration (FR-011)

Instead of modifying existing agent files, we use co-located instruction files that AI assistants automatically discover:

| File | AI Assistant | Discovery Mechanism |
|------|--------------|---------------------|
| `.specify/memory/feedback/AGENTS.md` | GitHub Copilot | "nearest AGENTS.md in directory tree" |
| `.specify/memory/feedback/CLAUDE.md` | Claude Code | "CLAUDE.md from child directories" |

### Agent Instruction Content (identical in both files)

```markdown
# Lessons Learned Integration

When working in this repository, incorporate lessons learned from past PR reviews.

## Instructions

1. Read `.specify/memory/lessons.md` if it exists
2. Filter lessons by relevance to current task:
   - For specifications: prioritize architecture, documentation lessons
   - For plans: prioritize architecture, testing, performance lessons
   - For implementation: prioritize code-quality, security, testing lessons
3. Apply relevant lessons to avoid repeating past mistakes
4. Reference applied lessons in output where appropriate

## Purpose

This database contains accumulated insights from PR reviews. Using it helps:
- Avoid repeating past mistakes
- Apply proven patterns
- Maintain code quality standards
```

**Benefits of this approach:**
- ✅ No modifications to existing speckit agent files
- ✅ Preserves repo-root CLAUDE.md for general rules
- ✅ Clean isolation - feedback instructions live with feedback data
- ✅ Cross-platform: works with both Copilot and Claude Code natively
