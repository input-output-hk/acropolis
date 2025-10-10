# spec-test Constitution
<!-- Minimal constitution defining baseline non-negotiables for the Rust project template. -->

## Core Principles

### I. Simplicity First
Keep the codebase as small and clear as possible. Prefer the standard library over external crates unless a dependency clearly reduces risk or complexity. Remove dead code immediately.

### II. Deterministic CLI
The binary must provide a single, deterministic, side‑effect‑free (except stdout/stderr) command interface. All output goes to stdout; errors (human readable) to stderr; exit codes are authoritative (0 success, non‑zero failure). No hidden network calls by default.

### III. Test Baseline (Non‑Negotiable)
Every new observable behavior requires at least one test (unit or integration). Minimum bar: `cargo test` green before merge. Panics in library-style functions are forbidden except for unrecoverable invariant breaks.

### IV. Safety & Lints
`#![forbid(unsafe_code)]` unless a justified, reviewed exception (documented inline with safety comment). Run `cargo clippy -- -D warnings` prior to merge. Zero warnings policy.

### V. Documentation Minimalism
Every public function has a one‑sentence rustdoc describing purpose + one example if non-trivial. `README.md` must show install, run, and example invocation.

## Project Structure Constraints
* Main binary crate (`processes/omnibus/src/main.rs`). If logic exceeds ~300 LOC or 3 responsibilities, refactor into internal modules or convert to workspace + library crate.
* Integration test "golden test" validation crate (`processes/golden_tests/src/lib.rs`)
* Dependencies must list rationale in a trailing comment inside `Cargo.toml` if not obviously standard (e.g., serde, anyhow, clap acceptable without comment once adopted).
* Build must succeed on stable Rust (pinned via `rust-toolchain.toml` when added later).
* No global mutable state; pass state explicitly.

## Workflow & Quality Gates
1. Format: `cargo fmt -- --check` must pass.
2. Lint: `cargo clippy -- -D warnings` must pass.
3. Test: `cargo test` must be green; add at least one integration test once behavior stabilizes.
4. Review: Every PR needs one reviewer confirming (a) no unjustified dependencies, (b) docs updated, (c) tests cover new branches.
5. Versioning: Start at 0.x; breaking changes permitted but must be documented in `CHANGELOG.md` once that file exists.
6. CI (future): Automate gates; local adherence required immediately.

## Governance
This constitution overrides ad-hoc preferences. Amendments require: (1) rationale summary, (2) impact note (tests, docs), (3) version bump below. Urgent security fixes may temporarily bypass a principle with a follow-up amendment PR within 48h.

Reviewers must block merges that: introduce unused abstractions, add dependencies without rationale, ignore lint/test failures, or reduce clarity.

Disagreements resolved by smallest-change principle: pick the option that solves the problem with least new code + deps.

**Version**: 0.1.0 | **Ratified**: 2025-10-08 | **Last Amended**: 2025-10-08
<!-- Initial version -->
