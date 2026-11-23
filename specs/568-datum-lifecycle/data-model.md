# Data Model: Datum Lifecycle Management

**Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)

## Entities

### ResolvedScript

Script bytecode resolved for Phase 2 evaluation.

| Field | Type | Description |
|-------|------|-------------|
| `hash` | `ScriptHash` | Script hash identifier |
| `language` | `ScriptLang` | V1, V2, or V3 |
| `bytecode` | `Vec<u8>` | CBOR-encoded flat Plutus script |

**Source**: Extracted from transaction witness set (raw tx bytes).

### ResolvedDatum

A datum resolved for a consumed UTxO.

| Field | Type | Description |
|-------|------|-------------|
| `hash` | `DatumHash` | Blake2b-256 hash of the datum |
| `bytes` | `Vec<u8>` | Raw CBOR-encoded PlutusData |
| `source` | `DatumSource` | How the datum was resolved |

**Validation**: `blake2b_256(bytes) == hash` must hold for witness-provided datums.

### DatumSource

Discriminated union of datum resolution paths.

| Variant | Fields | Description |
|---------|--------|-------------|
| `Inline` | — | Datum was inline in the UTxO output |
| `WitnessSet` | — | Datum found in transaction witness set by hash |

### ScriptContext

Version-polymorphic evaluation context for Plutus scripts.

| Variant | Structure (CBOR) | Application Model |
|---------|-----------------|-------------------|
| `V1` | `Constr(0, [TxInfo_v1, ScriptPurpose])` | `script(datum, redeemer, ctx)` |
| `V2` | `Constr(0, [TxInfo_v2, ScriptPurpose])` | `script(datum, redeemer, ctx)` |
| `V3` | `Constr(0, [TxInfo_v3, Redeemer, ScriptInfo])` | `script(ctx)` only |

### TxInfo (per version)

Core transaction information passed to Plutus scripts.

| Field | V1 | V2 | V3 | Type | Description |
|-------|:---:|:---:|:---:|------|-------------|
| `inputs` | ✓ | ✓ | ✓ | `[TxInInfo]` | Consumed UTxOs with resolved values |
| `outputs` | ✓ | ✓ | ✓ | `[TxOut]` | Produced outputs |
| `fee` | ✓ | ✓ | ✓ | `Value` | Transaction fee |
| `mint` | ✓ | ✓ | ✓ | `Value` | Minted/burned assets |
| `dcert` | ✓ | ✓ | — | `[DCert]` | Delegation certificates |
| `wdrl` | ✓ | ✓ | — | `Map<StakeCredential, Int>` | Reward withdrawals |
| `valid_range` | ✓ | ✓ | ✓ | `POSIXTimeRange` | Validity interval |
| `signatories` | ✓ | ✓ | ✓ | `[PubKeyHash]` | Required signers |
| `datums` | ✓ | ✓ | — | `Map<DatumHash, Datum>` | All datums in witness set |
| `id` | ✓ | ✓ | ✓ | `TxId` | Transaction hash |
| `reference_inputs` | — | ✓ | ✓ | `[TxInInfo]` | Reference inputs (CIP-31) |
| `redeemers` | — | ✓ | ✓ | `Map<ScriptPurpose, Redeemer>` | All redeemers |
| `votes` | — | — | ✓ | `Map<Voter, Map<GovActionId, Vote>>` | Governance votes |
| `proposal_procedures` | — | — | ✓ | `[ProposalProcedure]` | Governance proposals |
| `current_treasury_amount` | — | — | ✓ | `Option<Int>` | Treasury balance |
| `treasury_donation` | — | — | ✓ | `Option<Int>` | Treasury donation |

### ScriptPurpose / ScriptInfo

| Variant | Tag | Fields | Used In |
|---------|-----|--------|---------|
| `Spending` | 1 | `TxOutRef, Option<Datum>` | V1/V2/V3 |
| `Minting` | 0 | `CurrencySymbol` | V1/V2/V3 |
| `Certifying` | 3 | `Int, TxCert` | V2/V3 |
| `Rewarding` | 2 | `Credential` | V1/V2/V3 |
| `Voting` | 4 | `Voter` | V3 only |
| `Proposing` | 5 | `Int, ProposalProcedure` | V3 only |

**Note**: In V3, `ScriptInfo` replaces `ScriptPurpose`. For `Spending`, ScriptInfo includes the resolved datum directly (CIP-0069).

### Phase2Result

Result of running Phase 2 validation on a transaction.

| Field | Type | Description |
|-------|------|-------------|
| `valid` | `bool` | Whether all scripts passed |
| `scripts_run` | `usize` | Number of scripts evaluated |
| `total_cpu` | `u64` | Total ExUnits CPU consumed |
| `total_mem` | `u64` | Total ExUnits memory consumed |
| `error` | `Option<Phase2Error>` | First error encountered (if any) |

### Phase2Error

Detailed error information for failed Phase 2 validation.

| Variant | Fields | Description |
|---------|--------|-------------|
| `DatumNotFound` | `DatumHash` | Required datum hash not in UTxO or witness set |
| `DatumHashMismatch` | `DatumHash, DatumHash` | Computed hash ≠ declared hash |
| `ScriptNotFound` | `ScriptHash` | Referenced script not in witnesses or reference UTxOs |
| `EvaluationFailed` | `ScriptHash, String` | uplc-turbo evaluation error |
| `ExUnitsExceeded` | `ScriptHash, ExUnits, ExUnits` | Budget exceeded (used, limit) |
| `MissingRedeemer` | `ScriptPurpose` | No redeemer for script purpose |
| `CostModelNotFound` | `ScriptLang` | No cost model in protocol parameters |

## Relationships

```
TxUTxODeltas ──has──> raw_tx: Option<Vec<u8>>
     │
     ├──> consumes: Vec<UTxOIdentifier>  ──resolves──> UTXOValue
     │                                                    │
     │                                                    └──> datum: Option<Datum>
     │                                                              │
     │                                                              ├── Inline(bytes) ──> ResolvedDatum
     │                                                              └── Hash(h) ──lookup──> tx.plutus_data
     │                                                                                        │
     │                                                                                        └──> ResolvedDatum
     ├──> produces: Vec<(UTxOIdentifier, UTXOValue)>
     │
     └──> script_witnesses: Vec<(ScriptHash, ScriptLang)>
              │
              └──resolve──> ResolvedScript ──evaluate──> Phase2Result
                                │
                                └── needs: ScriptContext (version-specific)
                                             │
                                             ├── TxInfo (resolved UTxOs → TxInInfo)
                                             ├── Redeemer (from witness set)
                                             └── Datum (from ResolvedDatum)
```

## State Transitions

### Datum in UTxO Lifecycle

```
Created ──(tx produces output with datum)──> Stored
Stored  ──(tx consumes input)──────────────> Resolved (for Phase 2)
Resolved ──(Phase 2 pass)─────────────────> Consumed (UTxO spent)
Resolved ──(Phase 2 fail)─────────────────> Retained (collateral applied instead)
```

### Phase 2 Validation Flow (per transaction)

```
Start ──(collect input UTxOs)──> UTxOs Resolved
UTxOs Resolved ──(resolve datums per script)──> Datums Resolved
Datums Resolved ──(build ScriptContext per script)──> Contexts Built
Contexts Built ──(evaluate in parallel)──> Evaluation Complete
Evaluation Complete ──(all pass)──> Phase2Valid → apply normal inputs/outputs
Evaluation Complete ──(any fail)──> Phase2Invalid → apply collateral only
```

## Validation Rules

| Rule | Entity | Condition |
|------|--------|-----------|
| VR-1 | ResolvedDatum | `blake2b_256(bytes) == hash` for witness-set datums |
| VR-2 | ScriptContext | Version matches script language (V1→V1, V2→V2, V3→V3) |
| VR-3 | Phase2 | All scripts in transaction must pass for tx to be valid |
| VR-4 | Datum (V1/V2 Spending) | Datum must exist (inline or witness set) — error if missing |
| VR-5 | Datum (V3 Spending) | Datum may be absent (CIP-0069 allows it) |
| VR-6 | ExUnits | CPU and memory must not exceed per-script budget from redeemer |
| VR-7 | CostModel | Must exist in protocol parameters for the script language |
