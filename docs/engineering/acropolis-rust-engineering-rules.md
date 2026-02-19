# Acropolis Rust Engineering Rules

## 1. Purpose

This document defines engineering rules for Rust code in Acropolis to improve:

- reliability
- maintainability
- testability
- portability
- readability

These rules are intentionally multidimensional (layout, design, safety, dependencies, testing, operations).

`cargo fmt` and `cargo clippy` remain required project tooling, but this policy does not duplicate their rule sets.

## 2. Rule Levels and Deviations

Each rule is marked as one of:

- `should`: strongly recommended.
- `will`: mandatory by project convention; verified in review/checklists.
- `shall`: mandatory and must be verified (automatically or manually).

### 2.1 Deviation Process

- `should` deviation: approval by module owner or tech lead.
- `will`/`shall` deviation: approval by module owner and engineering lead.
- Every `shall` deviation shall be documented in the same file (or adjacent module README) with:
  - rule ID,
  - justification,
  - issue link,
  - expiration/revisit date.

### 2.2 Exceptions

If a rule defines an explicit exception, no additional deviation approval is required for that exception, but the exception use must still be documented in code comments when non-obvious.

## 3. General Design Rules

### 3.1 Coupling and Cohesion

- `ACR-001 (shall)` Each crate/module shall have a single clear responsibility.
- `ACR-002 (shall)` Cross-module contracts shall be expressed via shared types/messages in common crates; modules shall not reach into other modules' internals.
- `ACR-003 (will)` `processes/*` crates will only compose/wire modules and configuration; domain logic will live in library crates.
- `ACR-004 (should)` Hardware/protocol/third-party boundaries should be isolated behind adapter traits or focused integration modules.
- `ACR-005 (shall)` Global mutable state shall be avoided; where unavoidable, wrap in a constrained API with documented synchronization strategy.

### 3.2 API and Data Boundaries

- `ACR-006 (shall)` Public APIs shall minimize exposed surface area (prefer crate-private/private items by default).
- `ACR-007 (should)` Newtypes should be used at domain boundaries for IDs/hashes/units where misuse risk exists.
- `ACR-008 (shall)` Serialization/deserialization logic shall be centralized; wire-format assumptions shall not leak broadly across business logic.

## 4. Code Layout and Complexity

LSLOC below means non-empty, non-comment logical lines.

- `ACR-009 (will)` Any one function/method will contain no more than 100 LSLOC.
  - Exception: parser/state-machine match blocks may exceed this up to 160 LSLOC if extracted helper functions would reduce clarity.
- `ACR-010 (should)` Files should stay at or below 500 LSLOC.
- `ACR-011 (shall)` Files exceeding 800 LSLOC shall be split unless a documented exception exists.
- `ACR-012 (will)` Cyclomatic complexity per function will be <= 15.
  - Exception: exhaustive dispatch (`match`) over protocol enums may exceed this up to 25 with rationale.
- `ACR-013 (should)` Public functions and trait methods should expose domain-specific input structs when argument groups represent a concept, even when argument count is below lint thresholds.
- `ACR-014 (shall)` Nesting depth > 4 shall be refactored (guards/early returns/helper functions).

## 5. Safety and Correctness

- `ACR-015 (shall)` `unsafe` code shall be isolated, minimized, and accompanied by a safety comment documenting invariants.
- `ACR-016 (shall)` Panic paths (`panic!`, `unwrap`, `expect`, `unreachable!`, `todo!`) shall not appear in production paths except when explicitly proving impossible states; such uses require inline justification.
- `ACR-017 (shall)` Recoverable failures shall be modeled as `Result`/domain errors.
- `ACR-018 (will)` Numeric conversions will be explicit and checked where truncation/overflow is possible.
- `ACR-019 (shall)` Timeouts/retries/backoff shall be explicit at network and external I/O boundaries.
- `ACR-020 (shall)` Deterministic behavior shall be preserved for consensus-critical logic; nondeterministic sources (time/randomness/system ordering) shall be isolated and injected.

## 6. Concurrency and Async

- `ACR-021 (shall)` Blocking operations shall not run on async executors without explicit offloading (`spawn_blocking` or dedicated worker).
- `ACR-022 (will)` Every spawned task will have ownership/lifecycle semantics documented (who starts it, who stops it, and on what signal).
- `ACR-023 (shall)` Shared mutable state across tasks shall use bounded contention primitives and avoid lock hold across await points.
- `ACR-024 (should)` Bounded channels should be preferred over unbounded channels for backpressure-sensitive paths.

## 7. Dependencies and Libraries

- `ACR-025 (shall)` Dependencies shall be declared via workspace dependency management unless a justified exception is required.
- `ACR-026 (shall)` Adding a new third-party crate shall include:
  - purpose and alternatives considered,
  - maintenance posture (recent releases/activity),
  - license compatibility,
  - security review status.
- `ACR-027 (will)` Multiple crates solving the same concern (e.g., multiple async runtimes, logging stacks, JSON stacks) will be avoided.
- `ACR-028 (shall)` Critical-path crates (crypto/serialization/consensus/network) shall pin major versions deliberately and be upgraded through planned reviews.
- `ACR-029 (shall)` FFI or unsafe dependency usage shall be explicitly identified in crate docs and reviewed in PR.

## 8. Macros, Features, and Build Surface

- `ACR-030 (will)` Declarative macros will be preferred over procedural macros for local abstractions.
- `ACR-031 (shall)` Feature flags shall be additive and composable; mutually exclusive behaviors require explicit compile-time guards and tests.
- `ACR-032 (shall)` `cfg` conditionals shall be localized; cross-cutting conditional compilation that obscures behavior shall be avoided.
- `ACR-033 (should)` Generated code should be isolated in dedicated modules/files and not interleaved with hand-written core logic.

## 9. Error Handling, Logging, and Observability

- `ACR-034 (shall)` Error types shall preserve actionable context (operation, identifier, boundary).
- `ACR-035 (will)` Logs will use structured fields (hashes, slots, module names, topic names) rather than free-form-only text in critical flows.
- `ACR-036 (shall)` Consensus and state-transition events shall emit traceable identifiers for cross-module correlation.
- `ACR-037 (should)` Metrics should exist for throughput, backlog, latency, and error rates on network and pipeline boundaries.

## 10. Testing Rules

- `ACR-038 (shall)` Every bug fix shall include a regression test, unless technically impossible; in that case provide explicit rationale in PR.
- `ACR-039 (shall)` Public behavior changes shall include or update integration tests.
- `ACR-040 (will)` Deterministic pure logic will be covered by unit tests; IO/message-flow behavior will be covered by integration tests.
- `ACR-041 (should)` Property-based tests should be used for parsers, codecs, serialization, and state-machine transition invariants.
- `ACR-042 (shall)` Consensus/selection logic shall include fork/reorg/replay edge-case tests.
- `ACR-043 (will)` Coverage for changed lines will be demonstrated in CI reports for safety/consensus-critical crates.

## 11. Documentation Rules

- `ACR-044 (will)` Each crate will contain a concise README explaining purpose, boundaries, and message/data contracts.
- `ACR-045 (shall)` Public APIs and non-obvious invariants shall have rustdoc comments.
- `ACR-046 (should)` Architecture-significant decisions should be captured as ADR/spec entries and linked from PRs.

## 12. Compliance and Verification Matrix

The table below defines expected verification mode. A tool may change over time, but verification responsibility does not.

| Area | Example Rules | Verification Mode |
|---|---|---|
| Layout/complexity | ACR-009..014 | automatic metrics in CI + manual review for exceptions |
| Safety/panic/unsafe | ACR-015..020 | automatic scans + manual code review |
| Concurrency | ACR-021..024 | manual review + targeted tests |
| Dependencies | ACR-025..029 | dependency/security/license tooling + PR checklist |
| Features/macros | ACR-030..033 | manual review + compile matrix tests |
| Observability | ACR-034..037 | manual review + integration tests |
| Testing | ACR-038..043 | CI test gates + reviewer checklist |
| Documentation | ACR-044..046 | PR checklist/review |

## 13. Suggested Non-Redundant Enforcement Tooling

These complement (not duplicate) `fmt`/`clippy`:

- dependency and license policy (`cargo deny`)
- vulnerability monitoring (`cargo audit`)
- unused dependency checks (`cargo udeps`)
- unsafe usage inventory (`cargo geiger`)
- test execution and reporting (`cargo test`/`cargo nextest`)
- coverage reporting for changed code (`cargo llvm-cov` or equivalent)
- optional complexity/LOC reporting via custom scripts or metrics tooling

---

This policy should be versioned and reviewed quarterly. Thresholds can be tuned per crate criticality, but `shall` rules require formal deviation when not met.
