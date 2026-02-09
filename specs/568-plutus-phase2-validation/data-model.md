# Data Model: Plutus Phase 2 Validation

**Feature**: 568-plutus-phase2-validation  
**Date**: 2026-02-06

## Entity Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Phase 2 Validation Domain                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌───────────────┐     ┌───────────────┐                   │
│  │ PlutusScript  │────▶│ ScriptContext │                   │
│  └───────────────┘     └───────────────┘                   │
│         │                     │                             │
│         │                     │                             │
│         ▼                     ▼                             │
│  ┌───────────────┐     ┌───────────────┐                   │
│  │  EvalRequest  │────▶│  EvalResult   │                   │
│  └───────────────┘     └───────────────┘                   │
│                              │                              │
│                              ▼                              │
│                       ┌───────────────┐                    │
│                       │ Phase2Error   │                    │
│                       └───────────────┘                    │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## Core Entities

### 1. PlutusScript

Represents an executable Plutus smart contract.

| Field | Type | Description |
|-------|------|-------------|
| `script_hash` | `ScriptHash` | 28-byte Blake2b-224 hash identifying the script |
| `version` | `PlutusVersion` | Language version (V1, V2, V3) |
| `bytes` | `Vec<u8>` | CBOR-encoded FLAT bytecode |

**Validation Rules**:
- `bytes` must decode successfully via `uplc_turbo::flat::decode()`
- `version` determines which cost model and ScriptContext format to use
- `script_hash` must match the hash of the decoded script

**State Transitions**: None (immutable once witnessed in transaction)

```rust
#[derive(Debug, Clone)]
pub struct PlutusScript {
    pub script_hash: ScriptHash,
    pub version: PlutusVersion,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlutusVersion {
    V1,
    V2,
    V3,
}
```

---

### 2. ScriptPurpose

Identifies why a script is being executed.

| Variant | Fields | Description |
|---------|--------|-------------|
| `Spending` | `tx_in: TransactionInput` | Spending a UTxO locked by script |
| `Minting` | `policy_id: PolicyId` | Minting/burning tokens |
| `Certifying` | `cert_index: u32, cert: Certificate` | Delegation/registration cert |
| `Rewarding` | `reward_address: RewardAddress` | Withdrawing staking rewards |
| `Voting` | `voter: Voter` | Governance voting (V3 only) |
| `Proposing` | `proposal_index: u32, proposal: ProposalProcedure` | Governance proposal (V3 only) |

```rust
#[derive(Debug, Clone)]
pub enum ScriptPurpose {
    Spending(TransactionInput),
    Minting(PolicyId),
    Certifying { index: u32, certificate: Certificate },
    Rewarding(RewardAddress),
    Voting(Voter),
    Proposing { index: u32, procedure: ProposalProcedure },
}
```

---

### 3. ScriptContext

Complete context provided to a script for evaluation.

| Field | Type | Description |
|-------|------|-------------|
| `tx_info` | `TxInfo` | Transaction body information |
| `purpose` | `ScriptPurpose` | Why this script is running |

**Version Differences**:
- V1: Basic TxInfo (inputs, outputs, mint, fee, signatories)
- V2: Adds reference_inputs, inline_datums, redeemers
- V3: Adds voting_procedures, proposal_procedures, treasury

```rust
#[derive(Debug, Clone)]
pub struct ScriptContext {
    pub tx_info: TxInfo,
    pub purpose: ScriptPurpose,
}
```

---

### 4. TxInfo

Transaction information exposed to scripts.

| Field | Type | V1 | V2 | V3 |
|-------|------|----|----|-----|
| `inputs` | `Vec<TxInInfo>` | ✓ | ✓ | ✓ |
| `reference_inputs` | `Vec<TxInInfo>` | ✗ | ✓ | ✓ |
| `outputs` | `Vec<TxOut>` | ✓ | ✓ | ✓ |
| `fee` | `Value` | ✓ | ✓ | ✓ |
| `mint` | `Value` | ✓ | ✓ | ✓ |
| `certificates` | `Vec<Certificate>` | ✓ | ✓ | ✓ |
| `withdrawals` | `Map<Credential, Lovelace>` | ✓ | ✓ | ✓ |
| `valid_range` | `POSIXTimeRange` | ✓ | ✓ | ✓ |
| `signatories` | `Vec<PubKeyHash>` | ✓ | ✓ | ✓ |
| `redeemers` | `Map<ScriptPurpose, Redeemer>` | ✗ | ✓ | ✓ |
| `datums` | `Map<DatumHash, Datum>` | ✓ | ✓ | ✓ |
| `id` | `TxId` | ✓ | ✓ | ✓ |
| `votes` | `Map<Voter, Map<GovernanceActionId, Vote>>` | ✗ | ✗ | ✓ |
| `proposal_procedures` | `Vec<ProposalProcedure>` | ✗ | ✗ | ✓ |
| `current_treasury` | `Option<Lovelace>` | ✗ | ✗ | ✓ |
| `treasury_donation` | `Option<Lovelace>` | ✗ | ✗ | ✓ |

---

### 5. ExBudget

Execution resource limits.

| Field | Type | Description |
|-------|------|-------------|
| `cpu` | `i64` | CPU steps budget |
| `mem` | `i64` | Memory units budget |

```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct ExBudget {
    pub cpu: i64,
    pub mem: i64,
}

impl ExBudget {
    pub fn from_protocol_params(params: &ProtocolParams) -> Self {
        Self {
            cpu: params.max_tx_ex_units.steps as i64,
            mem: params.max_tx_ex_units.mem as i64,
        }
    }
}
```

---

### 6. EvalRequest

Request to evaluate a script.

| Field | Type | Description |
|-------|------|-------------|
| `script` | `PlutusScript` | Script to evaluate |
| `datum` | `Option<PlutusData>` | Datum (for spending scripts) |
| `redeemer` | `PlutusData` | Redeemer data |
| `context` | `ScriptContext` | Transaction context |
| `budget` | `ExBudget` | Execution limits |
| `cost_model` | `Vec<i64>` | Protocol parameter cost model |

```rust
#[derive(Debug)]
pub struct EvalRequest<'a> {
    pub script: &'a PlutusScript,
    pub datum: Option<&'a PlutusData>,
    pub redeemer: &'a PlutusData,
    pub context: &'a ScriptContext,
    pub budget: ExBudget,
    pub cost_model: &'a [i64],
}
```

---

### 7. EvalOutcome

Result of script evaluation.

| Variant | Fields | Description |
|---------|--------|-------------|
| `Success` | `consumed: ExBudget, logs: Vec<String>` | Script passed |
| `Failure` | `error: ScriptError, consumed: ExBudget, logs: Vec<String>` | Script failed |

```rust
#[derive(Debug)]
pub enum EvalOutcome {
    Success {
        consumed_budget: ExBudget,
        logs: Vec<String>,
    },
    Failure {
        error: ScriptError,
        consumed_budget: ExBudget,
        logs: Vec<String>,
    },
}
```

---

### 8. ScriptError

Detailed script failure reasons.

| Variant | Description |
|---------|-------------|
| `ExplicitError` | Script called `error` builtin |
| `BudgetExceeded { cpu: i64, mem: i64 }` | Exceeded execution limits |
| `DeserializationFailed { reason: String }` | Could not decode script |
| `TypeMismatch { expected: String, got: String }` | Argument type error |
| `MissingDatum { hash: DatumHash }` | Required datum not found |
| `MachineError { message: String }` | CEK machine error |

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScriptError {
    #[error("Script explicitly failed")]
    ExplicitError,
    
    #[error("Execution budget exceeded: cpu={cpu}, mem={mem}")]
    BudgetExceeded { cpu: i64, mem: i64 },
    
    #[error("Script deserialization failed: {reason}")]
    DeserializationFailed { reason: String },
    
    #[error("Type mismatch: expected {expected}, got {got}")]
    TypeMismatch { expected: String, got: String },
    
    #[error("Missing datum: {hash}")]
    MissingDatum { hash: String },
    
    #[error("Machine error: {message}")]
    MachineError { message: String },
}
```

---

### 9. Phase2ValidationError

Top-level validation error for reporting.

| Variant | Fields | Description |
|---------|--------|-------------|
| `ScriptFailed` | `script_hash, purpose, error` | A script evaluation failed |
| `MissingScript` | `script_hash` | Script not found in witnesses |
| `MissingRedeemer` | `purpose` | No redeemer for script |
| `InvalidRedeemerPointer` | `pointer` | Redeemer points to non-script |

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum Phase2ValidationError {
    #[error("Script {script_hash} failed for {purpose:?}: {error}")]
    ScriptFailed {
        script_hash: ScriptHash,
        purpose: ScriptPurpose,
        error: ScriptError,
    },
    
    #[error("Missing script: {script_hash}")]
    MissingScript { script_hash: ScriptHash },
    
    #[error("Missing redeemer for {purpose:?}")]
    MissingRedeemer { purpose: ScriptPurpose },
    
    #[error("Invalid redeemer pointer: {pointer:?}")]
    InvalidRedeemerPointer { pointer: RedeemerPointer },
}
```

---

## Relationships

```
Transaction
    │
    ├── witnesses
    │       ├── PlutusScript[] ──────────────┐
    │       ├── Redeemer[]                   │
    │       └── Datum[]                      │
    │                                        │
    └── body ────────────────────────────────┼──▶ TxInfo
            ├── inputs[] ──▶ ScriptPurpose::Spending
            ├── mint ──────▶ ScriptPurpose::Minting
            ├── certs[] ───▶ ScriptPurpose::Certifying
            ├── withdrawals ▶ ScriptPurpose::Rewarding
            ├── votes[] ───▶ ScriptPurpose::Voting
            └── proposals[] ▶ ScriptPurpose::Proposing
                                             │
                                             ▼
                                      ScriptContext
                                             │
                                             ▼
                                       EvalRequest
                                             │
                                             ▼
                               ┌─────────────┴─────────────┐
                               ▼                           ▼
                        EvalOutcome::Success        EvalOutcome::Failure
                                                           │
                                                           ▼
                                                  Phase2ValidationError
```

---

## Protocol Parameter Mapping

| ProtocolParams Field | Usage |
|---------------------|-------|
| `max_tx_ex_units.steps` | `ExBudget.cpu` limit per transaction |
| `max_tx_ex_units.mem` | `ExBudget.mem` limit per transaction |
| `plutus_v1_cost_model` | Cost model for V1 scripts |
| `plutus_v2_cost_model` | Cost model for V2 scripts |
| `plutus_v3_cost_model` | Cost model for V3 scripts |

---

## PlutusData Encoding

Scripts receive arguments as `PlutusData`. The encoding for ScriptContext:

```rust
// ScriptContext = Constr 0 [TxInfo, ScriptPurpose]
PlutusData::constr(arena, 0, &[
    tx_info.to_plutus_data(arena),
    purpose.to_plutus_data(arena),
])

// ScriptPurpose variants
// Spending = Constr 0 [TxOutRef]
// Minting = Constr 1 [CurrencySymbol]
// Certifying = Constr 2 [Index, Certificate]
// Rewarding = Constr 3 [Credential]
// Voting = Constr 4 [Voter]
// Proposing = Constr 5 [Index, ProposalProcedure]
```
