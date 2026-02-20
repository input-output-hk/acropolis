# Speckit Workflow System

## Overview

This skill provides knowledge about the Speckit feature development workflow system. Speckit uses a phase-based approach to software development with agent-driven workflows stored in `.github/agents/` and `.github/prompts/`.

## When to Use This Skill

Use this skill when:
- User invokes `/speckit.*` commands
- User asks about the speckit workflow or phases
- User wants to create specifications, plans, or implementations
- User references specs directories or feature branches

## Speckit Phases

The speckit workflow consists of these phases, each with a corresponding agent file:

### Core Workflow Phases

1. **specify** - Create or update feature specifications from natural language
   - Location: `.github/agents/speckit.specify.agent.md`
   - Creates: Feature spec in `specs/NNN-feature-name/spec.md`
   - Creates branch: `NNN-feature-name`
   - Output: Technology-agnostic requirements and success criteria

2. **clarify** - Clarify specification requirements
   - Location: `.github/agents/speckit.clarify.agent.md`
   - Purpose: Resolve ambiguities and [NEEDS CLARIFICATION] markers
   - Updates: Feature spec with clarifications

3. **plan** - Generate technical implementation plan
   - Location: `.github/agents/speckit.plan.agent.md`
   - Creates: `plan.md`, `research.md`, `data-model.md`, `contracts/`, `quickstart.md`
   - Updates: Agent context files (copilot-instructions.md, etc.)
   - Phases: Research (Phase 0), Design & Contracts (Phase 1)

4. **tasks** - Break plan into actionable tasks
   - Location: `.github/agents/speckit.tasks.agent.md`
   - Creates: `tasks.md` with task breakdown and dependencies
   - Organizes: Setup, Tests, Core, Integration, Polish phases

5. **checklist** - Create validation checklists
   - Location: `.github/agents/speckit.checklist.agent.md`
   - Creates: Domain-specific checklists in `checklists/` directory
   - Types: UX, security, performance, testing, etc.

6. **implement** - Execute the implementation plan
   - Location: `.github/agents/speckit.implement.agent.md`
   - Executes: All tasks from `tasks.md`
   - Validates: Checklist completion before proceeding
   - Follows: TDD approach with phase-by-phase execution

7. **feedback** - Extract lessons from PR feedback
   - Location: `.github/agents/speckit.feedback.agent.md`
   - Creates: `.specify/memory/feedback/pr-NNN-lessons.md`
   - Updates: `.specify/memory/lessons.md` central database
   - Categories: code-quality, architecture, testing, documentation, security, performance

### Supporting Agents

8. **analyze** - Analyze existing codebase or feature
   - Location: `.github/agents/speckit.analyze.agent.md`

9. **constitution** - Validate against project principles
   - Location: `.github/agents/speckit.constitution.agent.md`

10. **taskstoissues** - Convert tasks to GitHub issues
    - Location: `.github/agents/speckit.taskstoissues.agent.md`

## Directory Structure

When a feature is created, speckit creates this structure:

```
specs/NNN-feature-name/
├── spec.md                    # Feature specification (from /speckit.specify)
├── plan.md                    # Implementation plan (from /speckit.plan)
├── tasks.md                   # Task breakdown (from /speckit.tasks)
├── research.md                # Research findings (from /speckit.plan Phase 0)
├── data-model.md              # Entity models (from /speckit.plan Phase 1)
├── quickstart.md              # Integration guide (from /speckit.plan Phase 1)
├── contracts/                 # API contracts (from /speckit.plan Phase 1)
│   ├── openapi.yaml
│   └── graphql.schema
└── checklists/                # Validation checklists (from /speckit.checklist)
    ├── requirements.md
    ├── ux.md
    ├── security.md
    └── testing.md
```

## Key Scripts

Speckit uses bash scripts in `.specify/scripts/bash/`:

- `create-new-feature.sh` - Initialize new feature branch and spec
- `setup-plan.sh` - Prepare planning context
- `check-prerequisites.sh` - Validate workflow prerequisites
- `update-agent-context.sh` - Update AI agent context files
- `fetch-pr-feedback.sh` - Extract PR review comments

## How Agent Files Work

Agent files in `.github/agents/` follow this format:

```yaml
---
description: Agent description
handoffs:
  - label: Next Action Label
    agent: speckit.next
    prompt: Default prompt text
    send: true/false
---

## User Input
$ARGUMENTS

## Outline
[Detailed workflow instructions...]
```

When a slash command is invoked, read the corresponding agent file and execute its workflow.

## Best Practices

1. **Always read the agent file** - Don't assume, read `.github/agents/speckit.{phase}.agent.md` before executing
2. **Parse JSON output** - Scripts output JSON with paths and metadata
3. **Follow phase order** - specify → clarify → plan → tasks → checklist → implement → feedback
4. **Validate prerequisites** - Use `check-prerequisites.sh` to ensure required files exist
5. **Respect [NEEDS CLARIFICATION]** - Don't proceed with unclear requirements
6. **Update agent context** - Keep copilot-instructions.md and similar files current

## Error Handling

- Missing dependencies: Report missing tools (gh, git, bash)
- Prerequisites not met: Suggest running earlier phases
- Validation failures: Stop and report specific issues
- Script errors: Show stderr output and suggest fixes

## Integration Points

- **Git**: Creates feature branches, commits artifacts
- **GitHub CLI**: Fetches PR data, creates issues
- **VS Code**: Agents work with VS Code Chat Participants
- **Claude Code**: Slash commands invoke agents via this skill
