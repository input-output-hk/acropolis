---
pr_number: 606
pr_title: "Script and consensus architecture rework"
pr_url: "https://github.com/input-output-hk/acropolis/pull/606"
extracted_date: 2026-01-21
lesson_count: 4
---

# Lessons from PR #606: Script and consensus architecture rework

## Lessons Extracted

### L001 - Design for Rollback Scenarios

**Category**: architecture  
**Tags**: consensus, messaging, rollback

When designing message flows, consider rollback scenarios from the start. Add explicit "rescind" or "withdraw" messages to handle cases where all peers roll back to before a particular block. This prevents orphaned state in consensus trees.

---

### L002 - Question Component Responsibilities

**Category**: architecture  
**Tags**: consensus, chain-store, separation-of-concerns

Question component responsibilities early in design reviews. If a downstream component (like chain store) can achieve its goal by simply listening to an existing message stream, it may not need explicit changes—keeping designs simpler.

---

### L003 - Handle Immutable Data Sources

**Category**: architecture  
**Tags**: mithril, consensus, immutable-data

Immutable data sources (like Mithril snapshots) can skip interactive flows (offer/wanted) and go directly to "favoured chain" status. Design consensus to recognize immutable blocks and optionally skip validation for trusted sources.

---

### L004 - Consistent Message Naming Voice

**Category**: documentation  
**Tags**: naming, api-design, consistency

Use consistent grammatical voice for message naming. Prefer passive voice for state-change events (e.g., `.offered`, `.rescinded`) to clearly indicate something has happened rather than an imperative action.
