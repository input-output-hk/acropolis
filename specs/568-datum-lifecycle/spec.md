# Feature Specification: Datum Lifecycle Management for Phase 2 Plutus Validation

**Feature Branch**: `568-datum-lifecycle`  
**Created**: 2026-02-12  
**Status**: Draft  
**Input**: User description: "Complete the development of issue #568 such that we add updating of datums resulting from smart contract evaluation — datum resolution, ScriptContext construction, and datum state tracking."  
**Parent**: `568-plutus-phase2-validation`

## Background

Acropolis currently evaluates Plutus scripts during Phase 2 validation but cannot resolve the datums required by spending validators. In Cardano's Extended UTxO (eUTxO) model, a "datum" is arbitrary data attached to a UTxO locked by a script. When a transaction consumes a script-locked UTxO, the associated datum is provided to the Plutus validator as its first argument. The validator uses this datum (representing "current state") together with the redeemer ("action") and transaction context to determine whether the spend is valid.

"Datum updates" refers to the complete lifecycle: resolving datums from consumed UTxOs, providing them to Plutus scripts for validation, and tracking the new datums produced on output UTxOs. Since UTxOs are immutable, "updating a datum" means consuming a UTxO with an old datum and producing a new UTxO with a new datum — the Plutus script enforces that this transition is valid.

Two datum formats exist on Cardano:

- **Datum Hash** (Alonzo era, Plutus V1/V2): The UTxO output stores only a 32-byte hash. The actual datum bytes must be provided in the transaction's witness set when spending.
- **Inline Datum** (Babbage+ era, Plutus V2/V3): The UTxO output stores the full datum bytes directly. No witness set lookup is needed.

Currently, Phase 2 validation passes `None` for the datum argument because the `tx_unpacker` module has no access to the UTxO store. This means all spending validators that access their datum argument will fail. This specification addresses that gap.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Datum Resolution for Spending Validators (Priority: P1)

As a node operator, I want the Acropolis node to resolve datums from consumed UTxOs and provide them to Plutus spending validators, so that smart contracts that depend on their datum state are validated correctly.

**Why this priority**: Without datum resolution, no spending validator that reads its datum can pass evaluation. This is the most fundamental gap — the majority of real-world Plutus contracts (DEXes, lending protocols, governance) are spending validators that read their datum.

**Independent Test**: Submit a block containing a transaction that spends a script-locked UTxO with a known datum. Verify the spending validator receives the correct datum bytes and evaluates successfully.

**Acceptance Scenarios**:

1. **Given** a transaction spending a UTxO whose output contains an inline datum, **When** Phase 2 validation runs the spending validator, **Then** the validator receives the inline datum bytes as its first argument and evaluates correctly.
2. **Given** a transaction spending a UTxO whose output references a datum hash, and the transaction witness set contains the corresponding datum bytes, **When** Phase 2 validation runs the spending validator, **Then** the validator receives the witness-set datum bytes as its first argument and evaluates correctly.
3. **Given** a transaction spending a UTxO whose output references a datum hash, but the transaction witness set does NOT contain the corresponding datum, **When** Phase 2 validation attempts datum resolution, **Then** validation fails with a clear error identifying the missing datum hash.
4. **Given** a transaction spending a UTxO that has no datum (non-script address or Plutus V3 with CIP-0069 no-datum spending), **When** Phase 2 validation runs, **Then** the validator is invoked without a datum argument as appropriate for its Plutus version.

---

### User Story 2 — ScriptContext Construction with Resolved Inputs (Priority: P2)

As a node operator, I want the Acropolis node to build a complete and correct ScriptContext for each Plutus script, so that scripts can inspect the full transaction information including resolved input UTxOs and their datums.

**Why this priority**: Plutus scripts receive a `ScriptContext` containing `TxInfo` which includes a list of all transaction inputs with their resolved addresses, values, and datums. Many real-world scripts inspect other inputs besides their own (e.g., checking oracle data feeds, verifying co-signers). Without a complete ScriptContext, these scripts will fail or produce incorrect results.

**Independent Test**: Submit a block containing a transaction with multiple inputs (some script-locked with datums, some plain), and verify the ScriptContext provided to each validator contains the correct resolved information for all inputs.

**Acceptance Scenarios**:

1. **Given** a transaction with multiple inputs including script-locked UTxOs with datums, **When** Phase 2 validation builds the ScriptContext, **Then** the `txInfoInputs` field contains each input with its resolved address, value, datum, and reference script information.
2. **Given** a transaction with reference inputs (read-only inputs that are not consumed), **When** Phase 2 validation builds the ScriptContext, **Then** the `txInfoReferenceInputs` field contains the resolved reference inputs with their datums.
3. **Given** a transaction producing outputs with datums (both inline and hash-referenced), **When** Phase 2 validation builds the ScriptContext, **Then** the `txInfoOutputs` field correctly represents each output's datum.

---

### User Story 3 — UTxO State Integration for Phase 2 (Priority: P1)

As a node operator, I want Phase 2 validation to have access to the UTxO state so that datums and script addresses can be resolved from consumed inputs, enabling correct validation of spending scripts.

**Why this priority**: This is the architectural enabler for User Stories 1 and 2. Currently Phase 2 runs in a module without UTxO state access. Without this integration, datum resolution and ScriptContext construction are impossible. Per L007: Phase 2 validation must run sequentially after Phase 1 per transaction because Phase 2 results determine what gets applied (inputs vs collateral).

**Independent Test**: Verify that when Phase 2 validation runs, it can query the UTxO store for any input referenced in the transaction and receive the complete UTxO data including address, value, and datum.

**Acceptance Scenarios**:

1. **Given** Phase 2 validation is enabled, **When** a transaction with spending scripts is processed, **Then** Phase 2 validation can resolve each input's UTxO data (address, value, datum) from the UTxO store.
2. **Given** a transaction references a UTxO that does not exist in the store (already spent or never created), **When** Phase 2 validation attempts to resolve it, **Then** a clear error is reported identifying the missing UTxO.
3. **Given** two transactions in the same block where the second spends an output created by the first, **When** Phase 2 validation processes the second transaction, **Then** the newly created UTxO from the first transaction is available for resolution.

---

### Edge Cases

- What happens when a spending validator expects a datum but the consumed UTxO has no datum? The system MUST report a datum resolution error specific to the missing datum, distinguishing between "UTxO has no datum" and "datum hash not found in witness set."
- What happens when the datum bytes in the witness set do not hash to the expected datum hash? The system MUST reject the transaction with a datum hash mismatch error.
- How are datums handled for Plutus V3 scripts using CIP-0069 (spending scripts without datum arguments)? The system MUST allow spending without a datum for V3 scripts that opt into this pattern.
- What happens when a transaction has both regular inputs and reference inputs with datums? Regular inputs consume the UTxO and its datum, while reference inputs only read the UTxO — both must have their datums resolved for ScriptContext but only regular inputs are consumed.
- How does the system handle the transition between datum-hash outputs (Alonzo/Babbage) and inline-datum outputs (Babbage+)? Both formats MUST be supported simultaneously since the chain contains historical outputs of both types.
- What happens when multiple scripts in the same transaction need datum resolution? All datums MUST be resolved before any script evaluation begins, and per L008, scripts within the same transaction can run in parallel once inputs are resolved.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST resolve datums for consumed UTxOs locked by Plutus scripts. For datum-hash outputs, resolution MUST look up the datum hash in the transaction's witness set (`plutus_data`). For inline-datum outputs, resolution MUST use the datum bytes stored directly on the UTxO.
- **FR-002**: System MUST provide the resolved datum as the first argument to Plutus spending validators during Phase 2 evaluation.
- **FR-003**: System MUST report a clear validation error when a datum cannot be resolved — distinguishing between "UTxO has no datum," "datum hash not in witness set," and "datum hash mismatch."
- **FR-004**: System MUST construct a complete ScriptContext/TxInfo for each script that includes resolved input UTxOs with their addresses, values, and datums.
- **FR-005**: System MUST resolve script addresses from consumed UTxOs to determine which script to run for each spending redeemer, rather than inferring the script from transaction metadata alone.
- **FR-006**: System MUST resolve datums for reference inputs (read-only inputs used for data, not consumed) and include them in the ScriptContext's reference inputs field.
- **FR-007**: System MUST validate that datum bytes provided in the witness set hash correctly to the datum hash referenced by the UTxO output.
- **FR-008**: System MUST support both Alonzo-era datum-hash outputs and Babbage-era inline-datum outputs simultaneously, as the chain contains outputs of both types.
- **FR-009**: System MUST handle intra-block UTxO dependencies — when transaction B in a block spends an output created by transaction A earlier in the same block, the datum from A's output must be available for B's validation.
- **FR-010**: System MUST provide Phase 2 validation with access to the UTxO state, either by integrating Phase 2 into the module that owns the UTxO store or by providing a query interface.

### Key Entities

- **Datum**: Arbitrary CBOR-encoded data associated with a script-locked UTxO. Represented as either a hash reference or inline bytes. Used by Plutus validators to track contract state.
- **DatumHash**: A 32-byte Blake2b-256 hash of a datum's CBOR encoding. Used in pre-Babbage outputs to reference datums stored in the transaction witness set.
- **Inline Datum**: Full datum bytes stored directly in a UTxO output (Babbage+ era). Does not require witness set lookup.
- **Witness Set Datum**: Datum bytes included in a transaction's witness set, keyed by their hash. Required when spending UTxOs that reference datums by hash.
- **Resolved Input**: A transaction input paired with its full UTxO data (address, value, datum, reference script) from the UTxO store. Required for ScriptContext construction and datum resolution.
- **ScriptContext**: The complete transaction context provided to a Plutus script, including resolved inputs, outputs, minting info, fee, validity range, signatories, redeemers, and datums. Its structure varies by Plutus version (V1/V2/V3).

## Assumptions

- The existing UTxO state module already stores datums (both hash and inline variants) as part of its UTxO entries, as verified by codebase research.
- The existing Phase 2 evaluator correctly accepts an optional datum parameter and applies it to spending validators — only the resolution of that datum is missing.
- Protocol parameters and cost models are already available from the existing Phase 1 validation context.
- The existing `Transaction` type already carries witness set datums as a list of (DatumHash, bytes) pairs.
- Per L007, Phase 2 validation runs sequentially after Phase 1 per transaction, and Phase 2 results determine whether inputs or collateral get applied to the UTxO state.
- The Cardano ledger rules for datum validation (Alonzo UTxOW rules) are well-defined and publicly documented.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of spending validators in the test corpus that require a datum argument receive the correct datum bytes (verified by comparing evaluation results against a reference Cardano node using the same inputs and protocol parameters).
- **SC-002**: 100% of transactions with datum-hash outputs correctly resolve datums from the witness set, and 100% of transactions with inline-datum outputs correctly use the inline bytes — verified by processing a mixed corpus of Alonzo-era and Babbage-era transactions.
- **SC-003**: Datum hash mismatches (witness set datum bytes don't hash to the expected hash) are detected and rejected with a specific error in 100% of test cases.
- **SC-004**: ScriptContext provided to validators contains correct resolved input data (address, value, datum) for all transaction inputs — verified by comparing serialized ScriptContext bytes against a reference implementation for a test corpus of multi-input transactions.
- **SC-005**: Intra-block UTxO dependencies are handled correctly — a transaction spending an output created earlier in the same block resolves the datum from that output, verified by processing a test block containing chained transactions.
- **SC-006**: On the reference benchmark environment, datum resolution adds less than 1 millisecond of overhead per transaction to Phase 2 validation time (measured as the difference between Phase 2 with and without datum resolution enabled, averaged over 100 transactions from the test corpus).
