# Research: SpecKit Feedback Phase

**Feature**: 001-speckit-feedback-phase  
**Date**: 2026-01-20

## Research Tasks

1. GitHub CLI PR Data Access
2. Existing Agent Patterns  
3. Lessons File Format Schema

---

## 1. GitHub CLI PR Data Access

**Decision**: Use `gh pr view` and `gh api` commands for PR data retrieval

**Rationale**: 
- `gh pr view --json` provides structured access to PR metadata
- `gh api` allows direct GraphQL queries for review comments and discussions
- Both are standard GitHub CLI commands available in any environment with `gh` installed

**Commands discovered**:

```bash
# Get PR details including body/description
gh pr view <number> --json number,title,body,state,mergedAt,headRefName

# Get review comments via API  
gh api graphql -f query='
  query($owner: String!, $repo: String!, $number: Int!) {
    repository(owner: $owner, name: $repo) {
      pullRequest(number: $number) {
        reviews(first: 100) {
          nodes {
            body
            author { login }
            state
            comments(first: 50) {
              nodes {
                body
                path
                line
              }
            }
          }
        }
      }
    }
  }
' -f owner=<owner> -f repo=<repo> -F number=<num>

# Get PR comments (discussion thread)
gh pr view <number> --json comments

# Get current repo info
gh repo view --json owner,name
```

**Alternatives considered**:
- Direct GitHub REST API: More verbose, requires explicit token management
- GitHub Action: Not applicable for local VS Code agent workflow

---

## 2. Existing Agent Patterns

**Decision**: Follow the established pattern from speckit.clarify.agent.md

**Pattern structure**:

```markdown
---
description: <one-line description>
handoffs:
  - label: <next step label>
    agent: <next agent>
    prompt: <transition prompt>
---

## User Input

```text
$ARGUMENTS
```

## Outline

<Goal statement>

<Execution steps as numbered list>
```

**Key observations from existing agents**:

| Agent | Key Pattern | Applicable Here |
|-------|-------------|-----------------|
| speckit.clarify | Interactive Q&A loop, writes to spec file | Yes - lesson categorization could be interactive |
| speckit.specify | Creates files, runs setup scripts | Yes - creates lesson files |
| speckit.plan | Reads spec, generates plan artifacts | Yes - reads PR data, generates lessons |

**Common patterns to follow**:
- Use `.specify/scripts/bash/check-prerequisites.sh` for context when applicable
- Argument parsing via `$ARGUMENTS` block
- Clear stop conditions
- File writes after each significant action

---

## 3. Lessons File Format Schema

**Decision**: Markdown with YAML frontmatter blocks per lesson entry

**Schema for individual lesson**:

```yaml
---
lesson_id: L001
category: code-quality
tags: [rust, error-handling]
source: pr
source_ref: "PR #123"
date: 2026-01-20
frequency: 1
---

Prefer `expect()` over `unwrap()` with descriptive messages to improve debugging when panics occur.
```

**Schema for lessons database header** (`.specify/memory/lessons.md`):

```yaml
---
title: Lessons Learned Database
description: Accumulated lessons from PR reviews and manual entries
last_updated: 2026-01-20
total_lessons: 0
---

# Lessons Learned

This file contains accumulated lessons from PR reviews and manual entries.
Future speckit phases should read this file to avoid repeating past mistakes.

## Categories

- **code-quality**: Code style, idioms, best practices
- **architecture**: System design, patterns, structure
- **testing**: Test strategies, coverage, edge cases
- **documentation**: Comments, READMEs, API docs
- **security**: Auth, input validation, secrets
- **performance**: Optimization, efficiency
- **other**: Miscellaneous lessons

---

<!-- Lessons are appended below this line -->
```

**Rationale**:
- Human-readable markdown for easy manual review
- YAML frontmatter enables programmatic parsing by agents
- Frequency count supports deduplication consolidation
- Tags provide flexible secondary classification beyond fixed categories
- Clear section markers enable append-only updates

---

## Summary

| Research Area | Decision | Confidence |
|---------------|----------|------------|
| PR Data Access | GitHub CLI (`gh pr view`, `gh api`) | High |
| Agent Pattern | Follow speckit.clarify pattern | High |
| File Format | Markdown + YAML frontmatter | High |

All research tasks resolved. No NEEDS CLARIFICATION items remain.
