# Data Model: SpecKit Feedback Phase

**Feature**: 001-speckit-feedback-phase  
**Date**: 2026-01-20

## Entities

### Lesson

A discrete piece of feedback or learning extracted from a PR or entered manually.

| Attribute | Type | Required | Description |
|-----------|------|----------|-------------|
| lesson_id | string | Yes | Unique identifier (format: `L<number>`, e.g., `L001`) |
| content | string | Yes | The lesson text (markdown supported) |
| category | LessonCategory | Yes | Primary classification |
| tags | string[] | No | Optional free-form tags for additional context |
| source | "pr" \| "manual" | Yes | Origin of the lesson |
| source_ref | string | No | Reference to source (e.g., "PR #123", "Pair session 2026-01-20") |
| date | date | Yes | Date lesson was recorded (ISO 8601: YYYY-MM-DD) |
| frequency | integer | Yes | Number of times this lesson has been observed (default: 1) |

**Validation Rules**:
- `lesson_id` must be unique within the lessons database
- `frequency` must be ≥ 1
- `source_ref` required when `source` is "pr"

**State Transitions**: None (lessons are immutable once created; frequency may increment)

---

### LessonCategory (Enum)

Fixed classification for organizing lessons.

| Value | Description |
|-------|-------------|
| code-quality | Code style, idioms, language-specific best practices |
| architecture | System design, patterns, module structure |
| testing | Test strategies, coverage, edge case handling |
| documentation | Comments, READMEs, API documentation |
| security | Authentication, authorization, input validation, secrets handling |
| performance | Optimization, efficiency, resource usage |
| other | Miscellaneous lessons not fitting other categories |

---

### LessonsDatabase

The central aggregated file containing all lessons.

| Attribute | Type | Description |
|-----------|------|-------------|
| path | string | Fixed: `docs/feedback/lessons.md` |
| title | string | "Lessons Learned Database" |
| last_updated | date | Date of most recent update |
| total_lessons | integer | Count of lessons in database |
| lessons | Lesson[] | Ordered list of lessons (newest first within category) |

**File Format**: Markdown with YAML frontmatter header, followed by categorized lesson blocks.

**Update Rules**:
- Append-only: existing lessons are never deleted
- Duplicate detection: before adding, check for similar content (fuzzy match)
- If duplicate found: increment `frequency` on existing lesson instead of adding new
- Update `last_updated` and `total_lessons` on each modification

---

### PRLessonsFile

A per-PR file containing lessons extracted from a specific pull request.

| Attribute | Type | Description |
|-----------|------|-------------|
| path | string | `docs/feedback/pr-<number>-lessons.md` |
| pr_number | integer | GitHub PR number |
| pr_title | string | Title of the PR |
| pr_url | string | URL to the PR on GitHub |
| extracted_date | date | Date lessons were extracted |
| lessons | Lesson[] | Lessons extracted from this PR |

**File Format**: Markdown with YAML frontmatter containing PR metadata.

**Lifecycle**:
1. Created when `/speckit.feedback` runs for a PR
2. Updated incrementally on subsequent runs (new lessons merged with existing)
3. Duplicate detection: new lessons are compared to existing; duplicates are skipped, new lessons are appended

---

## Relationships

```
┌─────────────────────┐
│   LessonsDatabase   │
│  (docs/feedback/    │
│    lessons.md)      │
└──────────┬──────────┘
           │ contains (aggregated)
           │
           ▼
    ┌─────────────┐
    │   Lesson    │◄────────────────────┐
    └─────────────┘                     │
           ▲                            │
           │ contains (source copy)     │
           │                            │
┌──────────┴──────────┐                 │
│   PRLessonsFile     │                 │
│  (docs/feedback/    │                 │
│   pr-<N>-lessons.md)│                 │
└─────────────────────┘                 │
                                        │
                         ┌──────────────┴───┐
                         │  Manual Entry    │
                         │ (no file, direct │
                         │  to database)    │
                         └──────────────────┘
```

**Notes**:
- PRLessonsFile lessons are **copied** to LessonsDatabase (not referenced)
- Manual entries go directly to LessonsDatabase without a source file
- LessonsDatabase may contain lessons from many PRLessonsFiles

---

## Example File: Lesson Entry

Each lesson in the central database uses a markdown heading with a fenced YAML metadata block:

```markdown
### L042

```yaml
lesson_id: L042
category: code-quality
tags: [rust, error-handling, panic]
source: pr
source_ref: "PR #123"
date: 2026-01-20
frequency: 3
```

Prefer `expect("descriptive message")` over `unwrap()` to provide context when a panic occurs. This improves debugging by showing what operation failed and why it was unexpected.
```

**Note**: This format avoids ambiguous `---` YAML document separators between lessons. Each lesson is separated by blank lines.

---

## Example File: PR Lessons File

PR lessons files use the same fenced YAML format as the central database, but with simpler metadata (no source/frequency tracking since that's implicit in the PR file itself):

````markdown
---
pr_number: 123
pr_title: "Add MCP server support"
pr_url: "https://github.com/org/repo/pull/123"
extracted_date: 2026-01-20
lesson_count: 2
---

# Lessons from PR #123: Add MCP server support

## Lessons Extracted

### L042 - Use expect() over unwrap()

```yaml
category: code-quality
tags: [rust, error-handling, panic]
```

Prefer `expect("descriptive message")` over `unwrap()` to provide context when a panic occurs.


### L043 - Document MCP tool capabilities

```yaml
category: documentation
tags: [mcp, api]
```

Each MCP tool should have a clear description of what it does and example usage in its schema.
````
