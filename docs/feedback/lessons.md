---
title: Lessons Learned Database
description: Accumulated lessons from PR reviews and manual entries
last_updated: 2026-01-21
total_lessons: 4
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

---
lesson_id: L001
category: architecture
tags: [consensus, messaging, rollback]
source: pr
source_ref: "PR #606"
date: 2026-01-21
frequency: 2
---

When designing message flows, consider rollback scenarios from the start. Add explicit "rescind" or "withdraw" messages to handle cases where all peers roll back to before a particular block. This prevents orphaned state in consensus trees.

---

---
lesson_id: L002
category: architecture
tags: [consensus, chain-store, separation-of-concerns]
source: pr
source_ref: "PR #606"
date: 2026-01-21
frequency: 1
---

Question component responsibilities early in design reviews. If a downstream component (like chain store) can achieve its goal by simply listening to an existing message stream, it may not need explicit changes—keeping designs simpler.

---

---
lesson_id: L003
category: architecture
tags: [mithril, consensus, immutable-data]
source: pr
source_ref: "PR #606"
date: 2026-01-21
frequency: 1
---

Immutable data sources (like Mithril snapshots) can skip interactive flows (offer/wanted) and go directly to "favoured chain" status. Design consensus to recognize immutable blocks and optionally skip validation for trusted sources.

---

---
lesson_id: L004
category: documentation
tags: [naming, api-design, consistency]
source: pr
source_ref: "PR #606"
date: 2026-01-21
frequency: 1
---

Use consistent grammatical voice for message naming. Prefer passive voice for state-change events (e.g., `.offered`, `.rescinded`) to clearly indicate something has happened rather than an imperative action.
