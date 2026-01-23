---
title: Lessons Learned Database
description: Accumulated lessons from PR reviews and manual entries
last_updated: 2026-01-22
total_lessons: 9
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

<!-- Lessons are appended below this line -->

### L001

```yaml
lesson_id: L001
category: architecture
tags: [consensus, messaging, rollback]
source: pr
source_ref: "PR #606"
date: 2026-01-21
frequency: 2
```

When designing message flows, consider rollback scenarios from the start. Add explicit "rescind" or "withdraw" messages to handle cases where all peers roll back to before a particular block. This prevents orphaned state in consensus trees.


### L002

```yaml
lesson_id: L002
category: architecture
tags: [consensus, chain-store, separation-of-concerns]
source: pr
source_ref: "PR #606"
date: 2026-01-21
frequency: 1
```

Question component responsibilities early in design reviews. If a downstream component (like chain store) can achieve its goal by simply listening to an existing message stream, it may not need explicit changes—keeping designs simpler.


### L003

```yaml
lesson_id: L003
category: architecture
tags: [mithril, consensus, immutable-data]
source: pr
source_ref: "PR #606"
date: 2026-01-21
frequency: 1
```

Immutable data sources (like Mithril snapshots) can skip interactive flows (offer/wanted) and go directly to "favoured chain" status. Design consensus to recognize immutable blocks and optionally skip validation for trusted sources.


### L004

```yaml
lesson_id: L004
category: documentation
tags: [naming, api-design, consistency]
source: pr
source_ref: "PR #606"
date: 2026-01-21
frequency: 1
```

Use consistent grammatical voice for message naming. Prefer passive voice for state-change events (e.g., `.offered`, `.rescinded`) to clearly indicate something has happened rather than an imperative action.


### L005

```yaml
lesson_id: L005
category: documentation
tags: [data-model, consistency, format]
source: pr
source_ref: "PR #631"
date: 2026-01-22
frequency: 1
```

When implementing file formats, ensure the actual output matches the examples in the data-model specification. All lesson files should use fenced YAML code blocks for metadata to maintain consistency.


### L006

```yaml
lesson_id: L006
category: documentation
tags: [yaml, parsing, format]
source: pr
source_ref: "PR #631"
date: 2026-01-22
frequency: 1
```

Avoid using `---` separators between lessons as it creates ambiguous YAML parsing. Use blank lines between lessons and fenced YAML code blocks for metadata within each lesson.


### L007

```yaml
lesson_id: L007
category: code-quality
tags: [bash, dependencies, validation]
source: pr
source_ref: "PR #631"
date: 2026-01-22
frequency: 1
```

When a script depends on external CLI tools (like `jq`, `gh`, etc.), always verify they are installed before attempting to use them. Add checks like: `command -v jq >/dev/null 2>&1 || error "jq is required but not installed."`


### L008

```yaml
lesson_id: L008
category: documentation
tags: [specification, implementation, consistency]
source: pr
source_ref: "PR #631"
date: 2026-01-22
frequency: 1
```

When implementation behavior changes (e.g., from "never updated" to "incremental updates"), update the corresponding specification documents to reflect the new behavior. Spec-implementation drift causes confusion.


### L009

```yaml
lesson_id: L009
category: security
tags: [bash, command-injection, eval]
source: pr
source_ref: "PR #631"
date: 2026-01-22
frequency: 1
```

When using `eval $(function_that_outputs_assignments)`, ensure all interpolated values are escaped to prevent command injection. A malicious branch name or environment variable containing single quotes can break out of assignments and execute arbitrary commands.
