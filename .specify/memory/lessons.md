---
total_lessons: 8
last_updated: 2026-02-12
---

# Lessons Learned

Central database of lessons extracted from PR feedback and manual entries.

## Categories

| Category | Description |
|----------|-------------|
| code-quality | Code style, idioms, language-specific best practices |
| architecture | System design, patterns, module structure |
| testing | Test strategies, coverage, edge case handling |
| documentation | Comments, READMEs, API documentation |
| security | Authentication, authorization, input validation, secrets handling |
| performance | Optimization, efficiency, resource usage |
| other | Miscellaneous lessons not fitting other categories |

<!-- Lessons are appended below this line -->

### L001

```yaml
lesson_id: L001
category: architecture
tags: [dependencies, cargo, reproducibility]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Pin git dependencies to a specific commit or tag. Unpinned git dependencies make builds non-reproducible and can change underneath you. Also avoid adding implementation dependencies in a spec-only PR.

### L002

```yaml
lesson_id: L002
category: documentation
tags: [templates, spec, completeness]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Don't ship unfilled template files. If a document (e.g. plan.md) isn't ready, omit it from the PR rather than including placeholder content that could confuse reviewers.

### L003

```yaml
lesson_id: L003
category: documentation
tags: [checklists, consistency, spec]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Keep checklists consistent with actual document content. If a spec references specific implementation details (crate names, etc.), don't mark "no implementation details" as passing on the checklist.

### L004

```yaml
lesson_id: L004
category: testing
tags: [success-criteria, benchmarks, measurability]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Success criteria must be fully measurable. Define the benchmark environment (hardware/profile), a concrete test corpus (e.g. golden vectors), and precise definitions for subjective terms like "typical scripts" or "equivalent error semantics".

### L005

```yaml
lesson_id: L005
category: documentation
tags: [accuracy, codebase, research]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Verify codebase state before documenting existing patterns. Don't claim a pattern exists (e.g. "follows existing error wrapping") without checking the actual code — it may not exist yet.

### L006

```yaml
lesson_id: L006
category: code-quality
tags: [types, reuse, duplication]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Reuse existing types from common crates instead of defining new ones. Check `acropolis_common` for existing types (e.g. `ExUnits`, `Transaction`) before creating duplicates in feature modules.

### L007

```yaml
lesson_id: L007
category: architecture
tags: [validation, ordering, cardano, phase2]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Phase 2 validation must run sequentially after Phase 1 per transaction, not in a separate async module, because Phase 2 results determine what gets applied (inputs vs collateral). Within a single transaction, scripts can be parallelized.

### L008

```yaml
lesson_id: L008
category: architecture
tags: [parallelism, validation, cardano]
source: pr
source_ref: "PR #669"
date: 2026-02-12
frequency: 1
```

Parallelism scope for script validation: scripts within the same transaction can run in parallel, and scripts from transactions with independent script contexts can also be parallelized. But cross-transaction parallelism requires care since earlier tx results affect later tx inputs.
