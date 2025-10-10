# Feature Specification: Parse and Display Amaru Snapshot (Conway+)

**Feature Branch**: `cet/snapshot_parser`  
**Created**: 2025-10-10  
**Status**: Draft  
**Input**: User description: "Add an option to parse and display information from an Amaru formatted snapshot. We only need to handle data types for Conway and forward (epoch 505 and beyond). We need full parsing of the snapshot according to the docs/amaru-snapshot-structure.md and snapshot-format.md files."

## User Scenarios & Testing (mandatory)

### User Story 1 - View summary of an Amaru snapshot (Priority: P1)

An operator provides an Amaru-formatted snapshot file and requests a summary view that shows the snapshot’s key metadata and counts (e.g., epoch number, era, number of stake pools, DReps, proposals, UTxOs, and protocol parameter hash).

**Why this priority**: This enables a quick validation that a snapshot is correct and relevant (Conway+), unlocking immediate operational value.

**Independent Test**: Provide a known-good Conway-era snapshot file and verify that the summary shows correct epoch, era, and entity counts within expected ranges.

**Acceptance Scenarios**:

1. Given a valid Amaru snapshot for epoch ≥ 505, When the operator requests a summary, Then the tool displays epoch number, era, total counts for key entities (stake pools, DReps, UTxOs), and a human-readable protocol parameters digest.
2. Given a snapshot that is pre-Conway (< 505) or lacks Conway-required structures, When parsed, Then the tool clearly reports that pre-Conway snapshots are not supported.

---

### User Story 2 - Inspect detailed sections (Priority: P2)

An operator chooses specific sections to display (e.g., protocol parameters, governance items, stake pools) for deeper inspection of snapshot content.

**Why this priority**: Operators need targeted inspection to diagnose issues without scanning the entire snapshot output.

**Independent Test**: Request only protocol parameters from a valid snapshot and verify the details match an authoritative source (e.g., expected values for that epoch).

**Acceptance Scenarios**:

1. Given a valid snapshot, When the operator requests “protocol parameters,” Then full current protocol parameter values are displayed.
2. Given a valid snapshot, When the operator requests “governance” details, Then the tool displays counts and identifiers for proposals, committees, constitution reference, and DReps.

---

### User Story 3 - Validate snapshot integrity (Priority: P3)

An operator validates that the snapshot is complete and internally consistent (e.g., required sections are present, no expired governance actions in the dataset for the chosen scope).

**Why this priority**: Early detection of broken or incomplete snapshots prevents wasted time downstream.

**Independent Test**: Provide a snapshot with a deliberately corrupted section and verify that the tool identifies the issue and provides a clear error message indicating which section failed.

**Acceptance Scenarios**:

1. Given a snapshot with missing required Conway fields, When parsed, Then the tool reports a validation error naming the missing section.
2. Given a snapshot with unsupported future-era fields, When parsed, Then the tool ignores unknown fields and proceeds, while noting the presence of unknown data.

---

### User Story 4 - Bootstrap Acropolis node from snapshot (Priority: P1)

An operator starts the Acropolis node using a Conway+ snapshot as the source of truth. The system uses the previously defined snapshot parser to fully parse the data and then bootstraps the runtime by dispatching module-appropriate snapshot data to each participating module.

**Why this priority**: Enables rapid environment initialization without replaying the entire chain, a key operational value for recovery and testing.

**Independent Test**: Provide a known-good Conway snapshot, start the bootstrap, and verify that each module receives its expected data package and acknowledges readiness; verify that the node reports a consistent initialized state.

**Acceptance Scenarios**:

1. Given a valid Conway+ snapshot, When the node is instructed to bootstrap from the snapshot, Then the system parses the snapshot and publishes per-module bootstrap data (e.g., protocol parameters, governance items, stake pools, UTxO segments, accounts) and the node reaches an initialized state.
2. Given any module fails to acknowledge or rejects its bootstrap data, When bootstrapping, Then the system reports a clear error naming the module and does not proceed to a partial-running state without operator confirmation.

### Edge Cases

- Snapshot is from pre-Conway eras (epoch < 505): Should produce a clear, non-fatal message stating unsupported era.
- Snapshot includes unknown/future fields: Must be ignored with an informational note; core sections still parse.
- Snapshot file is very large (multi-GB): Parsing must remain responsive with visible progress and must meet performance targets (see Success Criteria). The parser must not stall; if no forward progress is detected, surface a clear warning.
- Snapshot file is corrupt or truncated: Produce a clear error indicating the first failing section and suggested next steps.
- Only a subset of sections present (e.g., parameters separated): Display what is available and clearly mark missing sections.
- Module bootstrap timeout or rejection: If a module does not acknowledge within a reasonable period (e.g., 5 seconds per module), surface a timeout error and fail the overall bootstrap pending operator action.

## Requirements (mandatory)

### Functional Requirements

- **FR-001 (Scope gating)**: The system MUST restrict parsing and display to Conway-era and forward snapshots (epoch ≥ 505), reporting a clear message for earlier snapshots.
- **FR-002 (Format compliance)**: The system MUST fully parse the documented sections of the Amaru snapshot per the current “amaru-snapshot-structure” and “snapshot-formats” documentation, for Conway-era content.
- **FR-003 (Summary view)**: The system MUST provide a summary view including at minimum: epoch number, era label, counts of stake pools, DReps, proposals, UTxOs, and a digest/identifier of current protocol parameters.
- **FR-004 (Section filtering)**: The system MUST allow users to request display of specific sections only (e.g., protocol parameters, governance, stake pools, accounts, UTxO) and output only those sections.
- **FR-005 (Graceful unknowns)**: The system MUST ignore unknown/future fields and continue parsing when possible, noting their presence in the output.
- **FR-006 (Validation errors)**: The system MUST report missing or malformed required sections with clear, actionable error messages that name the failing section.
- **FR-007 (Non-supported era)**: The system MUST detect pre-Conway snapshots and report them as unsupported without attempting partial parsing.
- **FR-008 (Human-readable output)**: The system MUST display information in a human-readable format suitable for operators.
- **FR-009 (Progress visibility)**: The system MUST provide visible progress indication during parsing, updating at least once per second on large files, and MUST detect and surface stalls (no progress for > 2 seconds) with a warning.
- **FR-010 (Performance bounds)**: The system MUST meet the performance targets defined in Success Criteria.
- **FR-011 (Determinism)**: For the same input snapshot, outputs MUST be deterministic and reproducible.
- **FR-012 (Internationalization)**: Text output SHOULD be in English; non-ASCII inputs (e.g., metadata) MUST be safely displayed or indicated.
- **FR-013 (Snapshot bootstrapping)**: The system MUST support bootstrapping the node from a valid Conway+ snapshot, initializing the runtime state without chain replay.
- **FR-014 (Module dispatch)**: After parsing, the system MUST dispatch module-appropriate data packages to each participating module to complete initialization.
- **FR-015 (Ordering & dependencies)**: The system MUST ensure logical ordering of dispatch (e.g., protocol parameters available before modules that depend on them).
- **FR-016 (Acknowledgments & timeouts)**: The system MUST require module acknowledgments and enforce a reasonable timeout per module (default 5 seconds) before failing the bootstrap with a clear error.
- **FR-017 (Atomicity of start)**: The system MUST avoid entering a partially-initialized running state if any critical module fails bootstrap; it MUST surface an actionable error and remain in a safe halted state.

### Constraints

- **C-001 (Output format)**: Output MUST be human-readable. Machine-readable output is out of scope for this feature.
- **C-002 (UTxO streaming)**: UTxO parsing MUST be streaming and read data in 16 MB chunks.

### Key Entities (data involved)

- **Snapshot**: A self-contained file representing ledger state at a specific point; includes metadata (epoch, era) and multiple sections.
- **Epoch & Era**: Snapshot’s epoch number and era classification (Conway+ only in scope).
- **Protocol Parameters**: Current parameter set effective at the snapshot point.
- **Governance Data**: Proposals (with states), constitutional committee state, constitution reference, governance activity (e.g., dormant epochs), DReps.
- **Stake Pools**: Registered pools, updates, retirements.
- **Accounts**: Treasury, reserves, fees, and per-credential account entries.
- **UTxO Set**: Collection of transaction inputs to outputs at snapshot time.

## Success Criteria (mandatory)

### Measurable Outcomes

- **SC-001 (Coverage)**: Summary view displays at least 6 key metrics: epoch, era, protocol parameter digest, and counts for pools, DReps, UTxOs.
- **SC-002 (Correctness)**: On a curated test snapshot, entity counts and key values match an authoritative reference with 100% accuracy.
- **SC-003 (Robustness)**: For snapshots with unknown/future fields, parsing completes and notes unknowns; no crash or data loss in supported sections in 100% of test cases.
- **SC-004 (Error clarity)**: For corrupt or malformed snapshots, first failing section is identified in error messages in ≥ 95% of cases in test suite.
- **SC-005 (Determinism)**: Re-running the parse on the same file produces identical output 100% of the time.
- **SC-006 (Performance)**: A 2.5 GB snapshot file is parsed and summarized in under 5 seconds on standard operator hardware as defined for the test environment.
- **SC-007 (Progress)**: During large snapshot parsing, progress is updated at least once per second; if no progress occurs for more than 2 seconds, a stall warning is emitted.
- **SC-008 (Bootstrap completeness)**: Starting from a valid Conway+ snapshot, 100% of participating modules receive and acknowledge their bootstrap data, and the node reports an initialized state.
- **SC-009 (Bootstrap failure handling)**: If any module fails or times out, the system reports the offending module and halts bootstrap without entering a partial-running state, in 100% of negative test cases.

## Assumptions

- “Display” refers to human-readable console or log output for operators; machine-readable output may be added later if needed.
- Documentation in `docs/amaru-snapshot-structure.md` and `docs/snapshot-formats.md` reflects the source of truth for fields to extract in Conway+.
- Performance targets will be aligned with typical operator hardware once clarified.

## Decisions

- Output format: Human-readable only (no machine-readable output in this feature).
- Progress: Required, with periodic updates and stall detection.
- Performance: Must parse a 2.5 GB snapshot in under 5 seconds; UTxO parsing must operate in 16 MB streaming chunks.

## Test Plan (Appendix)

This appendix outlines concrete validation steps using existing fixtures and the manifest-generation script to verify requirements and success criteria. Steps are technology-agnostic and focus on observable outcomes.

### Test Inputs and Fixtures

- Conway-era snapshot fixture: Use the provided CBOR snapshot file located under `tests/fixtures/` (Conway+ epoch ≥ 505).
- Corrupted/truncated snapshot fixture: Use an intentionally corrupted snapshot in `tests/fixtures/` or create a truncated copy of the Conway fixture.
- Pre-Conway snapshot (if present): Any snapshot with epoch < 505 placed in `tests/fixtures/`.
- Manifest: Use the existing script to generate a human-readable manifest from the snapshot fixture as an authoritative reference for counts/fields.

### Environment Notes

- Performance measurements (SC-006) must be captured on the designated “standard operator hardware” for consistency (document CPU, RAM, storage type briefly in test notes).
- For determinism (SC-005), ensure a stable environment (no concurrent modifications to fixtures).

### Test Cases (mapped to Success Criteria)

1. SC-001 Coverage (Summary View)
   - Input: Valid Conway snapshot fixture.
   - Action: Request summary view.
   - Verify: Output includes epoch, era, protocol parameter digest, and counts for pools, DReps, UTxOs (≥ 6 metrics).

2. SC-002 Correctness (Cross-check with Manifest)
   - Input: Valid Conway snapshot and generated manifest.
   - Action: Parse snapshot; compare displayed counts/keys with manifest values.
   - Verify: 100% match of reported counts and key values.

3. SC-003 Robustness (Unknown/Future Fields)
   - Input: Snapshot containing additional/unknown fields (fixture or simulated).
   - Action: Parse snapshot.
   - Verify: Parsing completes; output notes unknown fields; supported sections remain intact.

4. SC-004 Error Clarity (Corrupt/Truncated)
   - Input: Corrupted or truncated snapshot fixture.
   - Action: Parse snapshot.
   - Verify: Error message identifies the first failing section with clear guidance; target ≥ 95% coverage across variations.

5. SC-005 Determinism
   - Input: Same valid snapshot fixture.
   - Action: Parse twice.
   - Verify: Outputs are byte-for-byte/text-for-text identical.

6. SC-006 Performance (2.5 GB / < 5s)
   - Input: 2.5 GB Conway-era snapshot.
   - Action: Measure elapsed time from parse start to summary displayed.
   - Verify: ≤ 5 seconds on standard operator hardware.

7. SC-007 Progress & Stall Detection
   - Input: Large snapshot.
   - Action: Start parsing; observe progress updates.
   - Verify: Updates at least once per second; if no forward progress > 2 seconds, a stall warning is emitted.

8. SC-008 Bootstrap Completeness
   - Input: Valid Conway snapshot; run bootstrap flow.
   - Action: Initiate bootstrap; observe module dispatches and acknowledgments.
   - Verify: 100% of participating modules acknowledge receipt; node transitions to initialized state.

9. SC-009 Bootstrap Failure Handling
   - Input: Induce a negative acknowledgment or no-ack in one module (test harness/config).
   - Action: Run bootstrap.
   - Verify: System identifies the offending module and halts without entering a partial-running state.

### Additional Functional Checks

- FR-007 Non-supported Era: With a pre-Conway snapshot, verify a clear unsupported-era message and no partial parsing.
- FR-004 Section Filtering: Request individual sections (e.g., protocol parameters only) and verify only the requested section is displayed.
- FR-009 Progress Visibility: Confirm progress behavior on large files aligns with requirements.
