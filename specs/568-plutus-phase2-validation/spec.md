# Feature Specification: Plutus Phase 2 Script Validation

**Feature Branch**: `568-plutus-phase2-validation`  
**Created**: 2026-02-06  
**Status**: Draft  
**Input**: User description: "Provide phase 2 validation of blocks by integrating a Plutus Language evaluator from pragma-org/uplc into the Acropolis system"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Single Script Validation (Priority: P1)

As a node operator, I want the Acropolis node to validate individual Plutus scripts within transactions so that I can trust that smart contract execution is correct before accepting blocks.

**Why this priority**: This is the core functionality required for Phase 2 validation. Without single script validation, no other features can work. It establishes the foundation for all Plutus validation.

**Independent Test**: Can be fully tested by submitting a block containing a transaction with a single Plutus script and verifying the validation result matches expected behavior.

**Acceptance Scenarios**:

1. **Given** a block containing a transaction with a valid Plutus script, **When** Phase 2 validation is enabled and the block is processed, **Then** the script evaluates successfully and the transaction is accepted.
2. **Given** a block containing a transaction with an invalid/failing Plutus script, **When** Phase 2 validation is enabled and the block is processed, **Then** the script evaluation fails with a descriptive error and the transaction is rejected.
3. **Given** a block containing a transaction with a Plutus script, **When** Phase 2 validation is disabled via configuration, **Then** script evaluation is skipped and the transaction proceeds based on Phase 1 validation only.

---

### User Story 2 - Multi-Script Block Validation (Priority: P2)

As a node operator, I want multiple Plutus scripts within a single block to be validated efficiently, preferably in parallel, so that block processing throughput is maintained even for script-heavy blocks.

**Why this priority**: Real-world blocks often contain multiple smart contract interactions. Efficient multi-script validation is essential for maintaining block processing performance.

**Independent Test**: Can be tested by submitting a block containing multiple transactions with Plutus scripts and measuring total validation time versus sequential baseline.

**Acceptance Scenarios**:

1. **Given** a block containing 10 transactions each with one Plutus script, **When** Phase 2 validation processes the block, **Then** all scripts are validated and the total validation time is less than processing them sequentially would require.
2. **Given** a block with multiple scripts where one fails validation, **When** Phase 2 validation processes the block, **Then** the failed script is identified with its specific error while other scripts complete their validation.

---

### User Story 3 - Configuration-Gated Validation (Priority: P3)

As a node operator, I want to enable or disable Phase 2 validation via a configuration setting so that I can control validation behavior without code changes and can safely roll out this feature.

**Why this priority**: Feature gating enables safe incremental rollout, testing in production-like environments, and provides an escape hatch if issues are discovered.

**Independent Test**: Can be tested by toggling the configuration flag and observing that script evaluation is included/excluded from block processing.

**Acceptance Scenarios**:

1. **Given** the Phase 2 validation configuration flag is set to disabled (default), **When** a block with Plutus scripts is processed, **Then** only Phase 1 validation is performed.
2. **Given** the Phase 2 validation configuration flag is set to enabled, **When** a block with Plutus scripts is processed, **Then** Phase 2 script validation runs after Phase 1 validation completes.
3. **Given** the configuration flag is changed at runtime (node restart), **When** the node restarts, **Then** the new validation behavior takes effect.

---

### Edge Cases

- What happens when a script exceeds execution budget limits? The system MUST reject the script with a budget exceeded error.
- What happens when a script references missing datum or redeemer data? The system MUST report a clear error identifying the missing data.
- How does the system handle malformed script bytes? The system MUST fail gracefully with a deserialization error rather than crashing.
- What happens during epoch boundary blocks that have special governance scripts? These MUST be validated using the same mechanism.
- How does the system handle scripts from different Plutus versions (V1, V2, V3)? The evaluator MUST support all Plutus versions present on mainnet.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST evaluate Plutus scripts using the UPLC evaluator from the pragma-org/uplc crate without modifications to that crate.
- **FR-002**: System MUST execute Phase 2 validation only after Phase 1 validation has passed successfully.
- **FR-003**: System MUST support validation of Plutus V1, V2, and V3 scripts.
- **FR-004**: System MUST provide a configuration flag to enable/disable Phase 2 validation, defaulting to disabled.
- **FR-005**: System MUST report Phase 2 validation failures with descriptive error information including script hash and failure reason.
- **FR-006**: System MUST validate scripts within the execution budget limits defined by protocol parameters.
- **FR-007**: System MUST provide script context (datum, redeemer, transaction context) to each script for evaluation.
- **FR-008**: System MUST support parallel execution of multiple scripts within a block when the feature is enabled.
- **FR-009**: System MUST maintain constant memory usage across multiple script evaluations (no memory leaks over time).
- **FR-010**: System MUST integrate with the existing validation outcome reporting mechanism to communicate Phase 2 results.

### Key Entities

- **Plutus Script**: An executable smart contract program in UPLC format. Has a script hash, version (V1/V2/V3), and CBOR-encoded bytecode.
- **Script Context**: The data required for script evaluation including the redeemer, datum (if applicable), and transaction purpose (spending, minting, certifying, rewarding, voting, proposing).
- **Execution Budget**: Resource limits (CPU steps and memory units) defining maximum script execution bounds.
- **Phase 2 Validation Result**: The outcome of script evaluation - either success or failure with error details.

## Assumptions

- The pragma-org/uplc crate provides a stable, compatible evaluation interface that can be called from Acropolis.
- The crate supports all Plutus language versions currently deployed on Cardano mainnet.
- Protocol parameters containing execution budgets are already available in the Acropolis system from Phase 1 validation context.
- The existing Phase 1 validation correctly identifies which transactions contain scripts requiring Phase 2 validation.
- The bump allocator used by the uplc crate handles memory efficiently without requiring manual cleanup between script evaluations.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On the reference benchmark environment (see below), individual script evaluation of the *Plutus Phase 2 Golden Corpus v1* completes in under 0.1 seconds (100 ms) per script at the 95th percentile.
- **SC-002**: On the reference benchmark environment, resident memory usage (RSS) of the Acropolis node remains within ±10% of the baseline measurement when processing a batch of 1,000 consecutive scripts from the *Plutus Phase 2 Golden Corpus v1*.
- **SC-003**: For a fixed multi-script test block derived from the *Plutus Phase 2 Golden Corpus v1*, end-to-end Phase 2 validation wall-clock time with parallel validation enabled is strictly less than the time to validate the same scripts sequentially in a single thread on the reference benchmark environment.
- **SC-004**: 100% of scripts that validate successfully on a reference Cardano node (same protocol parameters and ledger state) also validate successfully in Acropolis when using the same inputs and protocol parameters.
- **SC-005**: 100% of scripts that fail on a reference Cardano node (same protocol parameters and ledger state) also fail in Acropolis with the same failure classification (e.g., deserialization error, evaluation error, execution budget exhaustion) and, where an error code is provided, the same error code.
- **SC-006**: Node operators can enable or disable Phase 2 validation with a single configuration change and node restart, verified via an automated acceptance test that toggles the feature flag and observes changes in validation behavior.

**Benchmark Definitions**:

- **Reference benchmark environment**: A dedicated machine with a documented hardware and software profile (CPU model and core count, RAM size, storage type, operating system and version, and Acropolis build version). All latency and memory measurements for SC-001, SC-002, and SC-003 are taken on this environment under no other significant system load.
- **Plutus Phase 2 Golden Corpus v1**: A fixed, version-controlled set of Plutus scripts and associated transaction contexts, derived from mainnet and regression cases, used consistently across SC-001 through SC-005. “Typical scripts” in this specification refers exactly to the scripts in this corpus.
- **Failure classification semantics**: For SC-005, “equivalent error semantics” means that, for each failing script, Acropolis reports the same high-level failure category as the reference Cardano node (deserialization error vs. evaluation error vs. execution budget exhaustion vs. forbidden operation, etc.) and, where applicable, the same error code or tag, even if the free-form error message text differs.
