# Research: Parse and Display Amaru Snapshot (Conway+)

## Decisions

- Output format: Human-readable only; no JSON in this feature.
- Era gating: Reject pre-Conway snapshots (epoch < 505) with clear message.
- UTxO parsing: Stream in 16 MB chunks; avoid loading full set in memory.
- Progress/stall: Update progress ≥1 Hz; warn if no forward progress >2s.
- Performance target: 2.5 GB snapshot parsed to summary in < 5s on standard operator hardware.
- Bootstrap: After parse, dispatch per-module data; require acknowledgments; timeout 5s/module; atomic start (no partial running).

## Rationale

- Human-readable output aligns with operator workflows and spec constraints.
- Era gating keeps scope focused and reduces parsing complexity.
- Streaming UTxO limits memory usage and supports large files.
- Progress+stall detection improves operator trust and debuggability.
- Aggressive performance target motivates IO-efficient parsing and minimal allocations.
- Bootstrap dispatch mirrors existing modular architecture; acknowledgments ensure readiness.

## Alternatives Considered

- Machine-readable output (JSON): deferred to keep scope tight; can be added later.
- Full in-memory UTxO: rejected due to memory/latency overhead on large snapshots.
- No progress indicator: rejected; operators need visibility to detect stalls.
- Partial start on module failure: rejected to avoid undefined behavior; prefer safe halt.

## Open Items and References

- Validate exact field mapping against `docs/amaru-snapshot-structure.md` and `docs/snapshot-formats.md` during implementation.
- Confirm “standard operator hardware” profile used for performance tests.
