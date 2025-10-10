# Implementation Plan: Parse and Display Amaru Snapshot (Conway+)

**Branch**: `001-add-an-option` | **Date**: 2025-10-10 | **Spec**: /home/parallels/projects/acropolis/specs/001-add-an-option/spec.md
**Input**: Feature specification from `/specs/001-add-an-option/spec.md`

## Summary

Add an operator-facing option to parse and display information from Amaru-formatted snapshots (Conway+ only), with a streaming UTxO parser (16 MB chunks), visible progress and stall detection, and performance target of parsing a 2.5 GB snapshot in under 5 seconds. Extend functionality to bootstrap the Acropolis node from the parsed snapshot by dispatching per-module data and requiring acknowledgments.

## Technical Context

**Language/Version**: Rust (workspace, edition 2021)  
**Primary Dependencies**: serde (format decoding), bytes, memmap2 (existing), pallas (Point), anyhow, tracing; CLI/entrypoints via existing processes crates.  
**Storage**: Filesystem snapshots (CBOR per docs). No DB writes as part of display; bootstrap dispatch uses existing message bus.  
**Testing**: cargo test + integration tests under `tests/fixtures/` using provided Conway snapshot and manifest script for oracles.  
**Target Platform**: Linux server.  
**Project Type**: Multi-crate Rust workspace (common, modules, processes).  
**Performance Goals**: Parse 2.5 GB snapshot in < 5s; stream UTxOs at 16 MB per chunk; progress updates ≥1 Hz; stall warn if >2s without forward progress.  
**Constraints**: No pre-Conway parsing; output must be human-readable; deterministic outputs; zero panics on normal errors.  
**Scale/Scope**: Single-node operator workflows; offline snapshot analysis and node bootstrap from one snapshot.

Unknowns: None (clarified in spec). If further protocol nuances arise, capture as follow-ups.

## Constitution Check

Gate mapping against spec-test Constitution:

- Simplicity First: Prefer existing crates; avoid new heavy deps; use streaming and std IO. PASS (no unjustified deps added).
- Deterministic CLI: Output to stdout; errors to stderr; deterministic outputs. Ensure no hidden network calls during parse. PASS with guardrails in CLI design.
- Test Baseline: Add integration tests for summary, sections, errors, performance smoke; forbid panics. PASS (to implement).
- Safety & Lints: Avoid unsafe; leverage memmap2 safely (existing); clippy -D warnings, fmt. PASS (to enforce in CI/local).
- Documentation Minimalism: Add rustdoc for public APIs and README usage examples. PASS (to implement).
- Structure constraints: Keep logic inside common + processes; no global mutable state. PASS.

Re-check post-design: Ensure any new crates include rationale in Cargo.toml comments.

### Documentation (this feature)

```
specs/[###-feature]/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)
<!--
  ACTION REQUIRED: Replace the placeholder tree below with the concrete layout
  for this feature. Delete unused options and expand the chosen structure with
  real paths (e.g., apps/admin, packages/something). The delivered plan must
  not include Option labels.
-->

```
# [REMOVE IF UNUSED] Option 1: Single project (DEFAULT)
src/
├── models/
├── services/
├── cli/
└── lib/

tests/
├── contract/
├── integration/
└── unit/

# [REMOVE IF UNUSED] Option 2: Web application (when "frontend" + "backend" detected)
backend/
├── src/
│   ├── models/
│   ├── services/
│   └── api/
└── tests/

frontend/
├── src/
│   ├── components/
│   ├── pages/
│   └── services/
└── tests/

# [REMOVE IF UNUSED] Option 3: Mobile + API (when "iOS/Android" detected)
api/
└── [same as backend above]

ios/ or android/
└── [platform-specific structure: feature modules, UI flows, platform tests]
```

**Structure Decision**: Use existing workspace structure. New/updated code primarily in:

- `common/src/snapshot.rs`: Conway+ snapshot parsing & display utilities (human-readable formatting, progress hooks, streaming UTxO reader).
- `processes/omnibus` (or relevant CLI): Add option/command to invoke parsing and optional bootstrap; ensure deterministic stdout/stderr.
- `modules/*`: No structural changes; bootstrap dispatch uses existing bus; add handlers only if needed to accept bootstrap messages in tests.

Tests under `tests/` with fixtures and manifest oracle.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| None | N/A | N/A |

## Complexity Tracking

*Fill ONLY if Constitution Check has violations that must be justified*

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| [e.g., 4th project] | [current need] | [why 3 projects insufficient] |
| [e.g., Repository pattern] | [specific problem] | [why direct DB access insufficient] |
