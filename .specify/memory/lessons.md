---
title: Lessons Learned Database
description: Accumulated lessons from PR reviews and manual entries
last_updated: 2026-02-12
total_lessons: 19
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


### L010

```yaml
lesson_id: L010
category: security
tags: [database, input-validation, sql-injection]
source: manual
source_ref: "Manual entry 2026-01-22"
date: 2026-01-22
frequency: 1
```

Always validate user input before passing to database queries. This prevents SQL injection and other database-related security vulnerabilities.


### L011

```yaml
lesson_id: L011
category: other
tags: [bash, regex, quoting, sed]
source: manual
source_ref: "Manual entry 2026-01-22"
date: 2026-01-22
frequency: 1
```

Use `'\''` rather than `'` in regular expressions within shell scripts. This escape sequence (close quote, escaped quote, open quote) allows embedding literal single quotes in single-quoted strings.


### L012

```yaml
lesson_id: L012
category: architecture
tags: [dependencies, cargo, reproducibility]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Pin git dependencies to a specific commit or tag. Unpinned git dependencies make builds non-reproducible and can change underneath you. Also avoid adding implementation dependencies in a spec-only PR.


### L013

```yaml
lesson_id: L013
category: documentation
tags: [templates, spec, completeness]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Don't ship unfilled template files. If a document (e.g. plan.md) isn't ready, omit it from the PR rather than including placeholder content that could confuse reviewers.


### L014

```yaml
lesson_id: L014
category: documentation
tags: [checklists, consistency, spec]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Keep checklists consistent with actual document content. If a spec references specific implementation details (crate names, etc.), don't mark "no implementation details" as passing on the checklist.


### L015

```yaml
lesson_id: L015
category: testing
tags: [success-criteria, benchmarks, measurability]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Success criteria must be fully measurable. Define the benchmark environment (hardware/profile), a concrete test corpus (e.g. golden vectors), and precise definitions for subjective terms like "typical scripts" or "equivalent error semantics".


### L016

```yaml
lesson_id: L016
category: documentation
tags: [accuracy, codebase, research]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Verify codebase state before documenting existing patterns. Don't claim a pattern exists (e.g. "follows existing error wrapping") without checking the actual code — it may not exist yet.


### L017

```yaml
lesson_id: L017
category: code-quality
tags: [types, reuse, duplication]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Reuse existing types from common crates instead of defining new ones. Check `acropolis_common` for existing types (e.g. `ExUnits`, `Transaction`) before creating duplicates in feature modules.


### L018

```yaml
lesson_id: L018
category: architecture
tags: [validation, ordering, cardano, phase2]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Phase 2 validation must run sequentially after Phase 1 per transaction, not in a separate async module, because Phase 2 results determine what gets applied (inputs vs collateral). Within a single transaction, scripts can be parallelized.


### L019

```yaml
lesson_id: L019
category: architecture
tags: [parallelism, validation, cardano]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Parallelism scope for script validation: scripts within the same transaction can run in parallel, and scripts from transactions with independent script contexts can also be parallelized. But cross-transaction parallelism requires care since earlier tx results affect later tx inputs.
