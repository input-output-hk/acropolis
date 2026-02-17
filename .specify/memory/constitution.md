<!--
  Sync Impact Report
  ==================
  Version change: 0.0.0 (unversioned) → 1.0.0
  Bump rationale: MAJOR — first formal constitution from template;
    replaces freeform document with structured, versioned principles.

  Modified principles (old → new):
    - "Code Style & Safety" (§3) → Principle I: Code Quality & Safety
    - "Testing" (§5) → Principle II: Testing Standards
    - (new) → Principle III: Interface & Experience Consistency
    - (new) → Principle IV: Performance & Reliability

  Added sections:
    - Core Principles (4 formal principles with rationale)
    - Technical Stack & Architecture (consolidated from old §1, §2)
    - Development Workflow & Quality Gates (new)
    - Governance (new)

  Removed sections:
    - "Documentation" (§4) — absorbed into Principle I

  Templates requiring updates:
    - .specify/templates/plan-template.md ✅ no update needed
      (Constitution Check section is already a dynamic gate)
    - .specify/templates/spec-template.md ✅ no update needed
      (Success Criteria section already supports performance metrics)
    - .specify/templates/tasks-template.md ✅ no update needed
      (Polish phase already covers performance and testing)

  Follow-up TODOs: none
-->
# Acropolis Constitution

## Core Principles

### I. Code Quality & Safety

All Rust code in the Acropolis workspace MUST satisfy these non-negotiable
rules:

- **Clippy clean**: `cargo clippy -D warnings` MUST pass with zero
  warnings. No `#[allow(...)]` attributes without a justifying comment.
- **No panics in library code**: `unwrap()`, `expect()`, and `panic!()`
  are forbidden outside of tests and top-level process entry points.
  Use `Result<T, E>` with the `?` operator; propagate errors via
  `thiserror` (typed) or `anyhow` (ad-hoc).
- **No unsafe**: `unsafe` blocks are prohibited unless required by FFI
  or a performance-critical hot path, and MUST include a `// SAFETY:`
  comment explaining the invariant.
- **Formatting**: All code MUST be formatted with `rustfmt`
  (`make fmt` / `make check`). CI MUST reject unformatted code.
- **Documentation**: Every public type, trait, and function MUST carry
  a `///` doc comment. Internal modules SHOULD have a `//!` module-level
  comment when purpose is non-obvious.
- **Idiomatic Rust**: Prefer standard library types, iterators, and
  pattern matching. Avoid reimplementing functionality available in
  existing dependencies (especially Pallas).

**Rationale**: A Cardano node processes real economic value. Panics,
undefined behavior, or silent data corruption are unacceptable. Strict
static analysis catches errors before they reach the chain.

### II. Testing Standards

Testing MUST follow these discipline rules:

- **TDD workflow**: Prefer Test-Driven Development — write the test,
  confirm it fails, then implement until it passes (red-green-refactor).
- **Unit tests**: Every module MUST include `#[cfg(test)]` unit tests
  covering its public API surface. Tests MUST be runnable with
  `cargo test -p <package>`.
- **Integration tests**: Features that span multiple modules MUST have
  integration tests under `tests/integration/`. These tests MUST be
  runnable in CI to detect regressions in nightly builds.
- **After every change**: Run the associated unit tests before
  committing. CI MUST run the full `make test` suite.
- **Test naming**: Test functions MUST use descriptive snake_case names
  that state the scenario and expected outcome, e.g.,
  `test_block_unpacker_rejects_invalid_cbor`.
- **No test pollution**: Tests MUST NOT depend on external network
  resources or mutable shared state. Use fixtures, mocks, or
  deterministic replay data.

**Rationale**: Acropolis validates and produces blocks on a live
network. Regressions in consensus, state tracking, or serialization
can cause forks or lost funds. Automated tests are the primary safety
net.

### III. Interface & Experience Consistency

All external and internal interfaces MUST maintain consistency:

- **Message bus contracts**: Adding or modifying a `Message` variant
  in `common/src/messages.rs` MUST include a migration note and MUST
  NOT silently break existing subscribers. Deprecate before removing.
- **Module interface pattern**: Every module MUST follow the
  `#[module(...)]` macro pattern with `init`, subscribe via
  `context.message_bus`, and capture state in closures. Deviations
  require justification in the PR description.
- **REST API (Blockfrost)**: API responses MUST conform to the
  Blockfrost OpenAPI specification. Breaking changes require a version
  bump and a migration guide.
- **Configuration**: New module configuration MUST follow existing
  TOML conventions (`[module.<name>]` sections). Config keys MUST use
  kebab-case. Defaults MUST be sensible for mainnet operation.
- **Error messages**: User-visible errors (logs, API responses) MUST
  include enough context to diagnose the problem without reading
  source code. Prefer structured logging with key-value fields.

**Rationale**: Acropolis is consumed by downstream tools, wallets, and
operators. Inconsistent interfaces erode trust and increase integration
cost. Predictable patterns reduce onboarding time for contributors.

### IV. Performance & Reliability

Performance constraints for a Cardano node are non-negotiable:

- **Block processing latency**: Block validation and state application
  MUST complete within the slot time budget (1 second on mainnet).
  Hot paths MUST be profiled before and after significant changes.
- **Memory discipline**: Modules MUST NOT hold unbounded in-memory
  collections. Use streaming, pagination, or disk-backed storage
  (Fjall v3) for large datasets. Memory usage MUST be monitorable
  via metrics or logs.
- **Startup time**: Snapshot-based bootstrap MUST restore to a usable
  state within a reasonable wall-clock time. Genesis sync performance
  MUST be tracked across releases.
- **No blocking the bus**: Message bus handlers MUST NOT perform
  blocking I/O on the subscription thread. Offload heavy work to
  dedicated Tokio tasks. Slow subscribers MUST NOT back-pressure the
  entire system.
- **Concurrency safety**: All shared state MUST use `Arc<Mutex<_>>`,
  `Arc<RwLock<_>>`, or lock-free structures with documented ordering
  guarantees. Deadlock-prone lock hierarchies MUST be avoided.
- **Graceful degradation**: Modules MUST handle transient failures
  (network drops, peer disconnects) with retries and backoff rather
  than crashing.

**Rationale**: A Cardano node is a long-running, performance-sensitive
system. Missed slots, memory leaks, or cascading failures directly
impact the network and stake pool operators who depend on Acropolis.

## Technical Stack & Architecture

- **Language**: Rust (2021 edition, updated per workspace `Cargo.toml`)
- **Async runtime**: Tokio
- **Error handling**: `thiserror` for typed errors, `anyhow` for
  ad-hoc contexts
- **Serialization**: Serde (JSON/TOML), CBOR via Pallas
- **Cardano primitives**: Pallas (`pallas = "0.34.0"`)
- **Storage**: Fjall v3 for on-disk key-value state
- **Architecture**: Publish-subscribe message bus (Caryatid framework),
  modular single-process (in-memory) or multi-process (RabbitMQ)
- **Strict separation**: Public API in `lib.rs`, internal
  implementation hidden. All inter-module dependencies resolved at
  configuration/runtime, never at compile time.

## Development Workflow & Quality Gates

- **Pre-commit**: `make fmt && make clippy && make test` MUST pass
  before pushing. CI enforces the same checks.
- **Pull requests**: Every PR MUST include a description of what
  changed and why. PRs that modify message types, public APIs, or
  configuration MUST call out the change explicitly.
- **Code review**: At least one reviewer MUST approve before merge.
  Reviewers MUST verify compliance with this constitution's principles.
- **CI pipeline**: `make all` (format + clippy + test) runs on every
  PR. Failures block merge.
- **Commit discipline**: Prefer small, focused commits. Each commit
  SHOULD compile and pass tests independently.

## Governance

- This constitution supersedes all ad-hoc practices. Where existing
  code or documentation conflicts with these principles, the
  constitution is authoritative and the code SHOULD be updated to
  comply.
- **Amendments**: Any change to this constitution MUST be submitted as
  a PR with a clear rationale. The version MUST be incremented per
  semantic versioning (MAJOR for principle removals/redefinitions,
  MINOR for additions, PATCH for clarifications).
- **Compliance review**: PR reviewers MUST verify that changes comply
  with the active constitution. Violations MUST be flagged and resolved
  before merge.
- **Complexity justification**: Any deviation from these principles
  MUST be justified in the PR description and tracked in the plan's
  Complexity Tracking table.
- **Runtime guidance**: See `CLAUDE.md` for development environment
  setup, commands, and module authoring patterns.

**Version**: 1.0.0 | **Ratified**: 2026-02-16 | **Last Amended**: 2026-02-16
