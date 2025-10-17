# Phase 2 Tasks: Parse and Display Amaru Snapshot (Conway+)

Status: Planned — start implementation from CLI wiring and parser skeletons
Date: 2025-10-10 | Branch: cet-add-snapshot-parser

## Milestone A — CLI wiring and scaffolds

- [ ] Add CLI entrypoint in `processes/omnibus` for snapshot operations
  - [ ] Subcommands: `summary`, `sections`, `bootstrap`
  - [ ] Flags: `--file <path>`, `--params`, `--governance`, `--pools`, `--accounts`, `--utxo`
- [x] Add streaming snapshot parser `common/src/snapshot/streaming_snapshot.rs`
  - [x] Callback-based API for bootstrap process
  - [x] Parse UTXOs, pools, accounts, DReps from Conway snapshots
  - [x] Trait-based extensibility for state distribution
- [ ] Deterministic stdout/stderr formatting helpers

Deliverable: CLI prints summary/sections placeholders; exits without starting the runtime process.

## Milestone B — Parser implementation (Conway+)

- [ ] Snapshot header parsing (epoch/era detection; reject pre-Conway)
- [ ] Section readers (protocol params, governance, pools, accounts)
- [ ] UTxO streaming reader in 16 MB chunks
- [ ] Unknown/future field handling (ignore with note)
- [ ] Validation errors with named sections

Deliverable: `snapshot_summary` and `snapshot_sections` produce real values from fixtures.

## Milestone C — Progress & performance

- [ ] Progress ticker ≥1 Hz during large reads
- [ ] Stall detection if no progress >2s (warn)
- [ ] Performance smoke: 2.5 GB in <5s on target hardware (documented)

Deliverable: Meets SC-006/007 on documented hardware.

## Milestone D — Bootstrap flow

- [ ] Dispatch per-module bootstrap messages using existing bus
- [ ] Ordering guarantees (params before dependents)
- [ ] Acknowledgments and 5s per-module timeouts
- [ ] Halt on failure; clear error naming offending module

Deliverable: Node initializes from a valid snapshot; negative path halts safely.

## Testing

- [ ] Integration tests for summary, sections, errors, and bootstrap success/failure
- [ ] Determinism test (same file → identical output)
- [ ] Performance smoke test harness (skipped by default; documented)
- [ ] Fixtures: use `tests/fixtures/`; large local-only fixtures under `tests/fixtures/large/` (gitignored)

## Docs & DX

- [ ] Update `quickstart.md` with CLI usage examples and troubleshooting
- [ ] Add rustdoc on public APIs and link to docs/ snapshot structure
- [ ] Ensure `Cargo.toml` comments justify any new deps (keep minimal)
