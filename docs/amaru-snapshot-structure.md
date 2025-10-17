# Amaru Snapshot Structure Specification

## Overview

This document describes the internal structure of Amaru/Haskell Cardano node snapshots obtained via the `GetCBOR` ledger-state query. These snapshots represent the `EpochState` type from the Haskell implementation.

**Warning**: This structure is **not formally specified** and may change between Cardano node versions. This documentation is based on reverse-engineering the Amaru and Haskell node codebase(s) and empirical observation of Conway-era snapshots.

## Source References

- [Haskell EpochState definition](https://github.com/IntersectMBO/cardano-ledger/blob/33e90ea03447b44a389985ca2b158568e5f4ad65/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState/Types.hs#L121-L131)
- [Amaru snapshot parser](https://github.com/pragma-org/amaru) (import logic)

## Quick Reference: Common Data Locations

For quick access, here are the array navigation paths to commonly needed data:

| Data | Path | Type | Description |
|------|------|------|-------------|
| **Epoch Number** | `[0]` | u64 | Current epoch |
| **Treasury** | `[3][0][0]` | i64 | ADA in treasury (lovelace) |
| **Reserves** | `[3][0][1]` | i64 | ADA in reserves (lovelace) |
| **DReps** | `[3][1][0][0][0]` | Map | Delegated Representatives |
| **Committee Members** | `[3][1][0][0][1]` | Map | Constitutional committee |
| **Stake Pools** | `[3][1][0][1][0]` | Map | Registered stake pools |
| **Stake Accounts** | `[3][1][0][2][0][0]` | Map | Accounts with delegation/rewards |
| **UTXOs** | `[3][1][1][0]` | Map | All unspent transaction outputs |
| **Fees** | `[3][1][1][2]` | i64 | Accumulated fees |
| **Governance Proposals** | `[3][1][1][3][0][1]` | Vec | Active governance proposals |
| **Protocol Parameters** | `[3][1][1][3][3]` | Object | Current protocol parameters |
| **Rewards** | `[4][0][1][2]` | Map | Unclaimed rewards (conditional) |

**Note**: Rewards at `[4]` are only present if `has_rewards = true`, which is rare (epoch boundaries only).

## CBOR Structure

The snapshot is a CBOR-encoded array representing a `NewEpochState` with the following complete hierarchical structure:

```
TOP-LEVEL ARRAY:
[0] Epoch number (u64)
[1] Previous blocks made
[2] Current blocks made (BlockIssuers)
[3] Epoch State (ARRAY)
    [0] Account State (ARRAY)
        [0] treasury: i64
        [1] reserves: i64
    [1] Ledger State (ARRAY)
        [0] Cert State (ARRAY)
            [0] Voting State (ARRAY)
                [0] dreps: Map<DRepCredential, DRepState>
                [1] cc_members: Map<ColdCredential, HotCredential>
                [2] dormant_epoch: Epoch
            [1] Pool State (ARRAY)
                [0] pools: Map<PoolId, PoolParams>
                [1] pools_updates: Map<PoolId, PoolUpdate>
                [2] pools_retirements: Map<PoolId, Epoch>
                [3] deposits
            [2] Delegation State (ARRAY)
                [0] dsUnified (ARRAY)
                    [0] credentials: Map<StakeCredential, Account>
                    [1] pointers
                [1] dsFutureGenDelegs
                [2] dsGenDelegs
                [3] dsIRewards
        [1] UTxO State (ARRAY)
            [0] utxo: Map<TransactionInput, TransactionOutput>
            [1] deposited: u64
            [2] fees: i64
            [3] utxosGovState (ARRAY)
                [0] Proposals (ARRAY)
                    [0] roots (ARRAY)
                        [0] root_params
                        [1] root_hard_fork
                        [2] root_cc
                        [3] root_constitution
                    [1] proposals: Vec<ProposalState>
                [1] cc_state: ConstitutionalCommitteeState
                [2] constitution: Constitution
                [3] pparams: ProtocolParameters (current)
                [4] previous_pparams: ProtocolParameters
                [5] future_pparams: ProtocolParameters
                [6] DRep Pulsing State (ARRAY)
                    [0] Pulsing Snapshot (ARRAY)
                        [0] last_epoch_votes
                        [1] drep_distr
                        [2] drep_state
                        [3] pool_distr
                    [1] Ratify State (ARRAY)
                        [0] Enact State
                        [1] enacted: Vec<GovActionState>
                        [2] expired: Vec<ProposalId> (tagged)
                        [3] delayed: bool
            [4] utxosStakeDistr
            [5] utxosDonation
    [2] Snapshots (historical reward snapshots)
    [3] NonMyopic (stake pool ranking data)
[4] Rewards Update (CONDITIONAL - only if has_rewards) (ARRAY)
    [0] Pulsing State (ARRAY)
        [0] status: u32 (1 = complete)
        [1] Reward State (ARRAY)
            [0] delta_treasury: i64
            [1] delta_reserves: i64
            [2] rewards: Map<StakeCredential, Set<Reward>>
            [3] delta_fees: i64
    [1] NonMyopic
[5] Unknown (skipped if has_rewards is false)
[6] Unknown (skipped if has_rewards is false)
```

**Navigation Paths (Zero-Indexed)**:
- Treasury/Reserves: `[3][0][0]` and `[3][0][1]`
- DReps: `[3][1][0][0][0]`
- Committee Members: `[3][1][0][0][1]`
- Stake Pools: `[3][1][0][1][0]`
- Stake Accounts: `[3][1][0][2][0][0]`
- UTXOs: `[3][1][1][0]`
- Governance Proposals: `[3][1][1][3][0][1]`
- Rewards (conditional): `[4][0][1][2]`

### Element [0]: Epoch Number

**Type**: `u64`

**Description**: The epoch number of this snapshot.

**Example**: `507` (Conway era, mainnet)

**Validation**: For Conway+ era support, must be >= 505.

```rust
let epoch: u64 = decoder.u64()?;
assert!(epoch >= 505, "Conway era or later required");
```

### Element [1]: Previous Blocks Made

**Type**: Various (typically skipped)

**Description**: Block production data from the previous epoch. Not typically needed for bootstrap.

**Parsing**: `decoder.skip()?`

### Element [2]: Current Blocks Made

**Type**: `BlockIssuers` structure

**Description**: Block production data for the current epoch. Maps stake pool IDs to block counts.

**Usage**: Needed for stake pool metrics and epoch transitions.

### Element [3]: Epoch State

**Type**: `Array[4]`

**Description**: The main ledger state container. Contains all UTXOs, delegations, governance, and protocol parameters.

#### Epoch State Structure

```
Epoch State Array[4]:
  [0] = Account State
  [1] = Ledger State
  [2] = Snapshots
  [3] = NonMyopic
```

#### Element [3][0]: Account State

**Type**: `Array[2]`

**Structure**:
```
[0] = treasury: i64    // ADA in treasury
[1] = reserves: i64    // ADA in reserves
```

**Example**:
```rust
let treasury: i64 = decoder.decode()?;  // e.g., 1_500_000_000_000_000 lovelace
let reserves: i64 = decoder.decode()?;  // e.g., 10_000_000_000_000_000 lovelace
```

#### Element [3][1]: Ledger State

**Type**: `Array[2]`

**Description**: The core ledger state containing UTXOs, delegations, and governance.

**Structure**:
```
Ledger State Array[2]:
  [0] = Cert State (certificates, delegations, pools)
  [1] = UTxO State (UTXOs, fees, governance)
```

##### Ledger State[0]: Cert State

**Type**: `Array[3]`

**Structure**:
```
Cert State Array[3]:
  [0] = Voting State
  [1] = Pool State
  [2] = Delegation State
```

###### Cert State[0]: Voting State

**Type**: `Array[3]`

**Structure**:
```
[0] = dreps: Map<DRepCredential, DRepState>
[1] = cc_members: Map<ColdCredential, HotCredential>  // Committee delegations
[2] = dormant_epoch: Epoch
```

**Example**:
```rust
let dreps: BTreeMap<DRepCredential, DRepState> = decoder.decode()?;
let cc_members: BTreeMap<ColdCredential, HotCredential> = decoder.decode()?;
let dormant_epoch: u64 = decoder.decode()?;

println!("DReps: {}, Committee: {}, Dormant epochs: {}",
    dreps.len(), cc_members.len(), dormant_epoch);
```

###### Cert State[1]: Pool State

**Type**: `Array[4+]`

**Structure**:
```
[0] = pools: Map<PoolId, PoolParams>
[1] = pools_updates: Map<PoolId, PoolUpdate>
[2] = pools_retirements: Map<PoolId, Epoch>
[3] = deposits: ...
```

**Usage**: Essential for stake pool operations, delegation, and rewards calculation.

###### Cert State[2]: Delegation State

**Type**: `Array[5+]`

**Structure**:
```
[0] = dsUnified (ARRAY):
      [0] = credentials: Map<StakeCredential, Account>
      [1] = pointers: ...
[1] = dsFutureGenDelegs: ...
[2] = dsGenDelegs: ...
[3] = dsIRewards: ...
[...]
```

**Key Field**: `credentials` contains all stake accounts with their delegation and reward state.

##### Ledger State[1]: UTxO State

**Type**: `Array[6]`

**Structure**:
```
UTxO State Array[6]:
  [0] = utxo: Map<TransactionInput, TransactionOutput>  ← THE ACTUAL UTXOs!
  [1] = deposited: u64
  [2] = fees: i64
  [3] = utxosGovState (governance proposals, committee, constitution, params)
  [4] = utxosStakeDistr
  [5] = utxosDonation
```

**Critical**: Element [0] is the UTXO map containing all unspent transaction outputs.

###### UTxO State[0]: UTXO Map

**Type**: `Map<TransactionInput, TransactionOutput>`

**Description**: The complete UTXO set. This is what you want for most use cases.

**Key Structure**:
```rust
// TransactionInput (key)
struct TxIn {
    tx_hash: [u8; 32],
    output_index: u64,
}
```

**Value Structure** (simplified):
```rust
// TransactionOutput (value)
struct TxOut {
    address: Address,      // Shelley/Byron address
    value: Value,          // ADA + native assets
    datum: Option<Datum>,  // Inline datum (Conway)
    script_ref: Option<Script>,  // Reference script (Conway)
}
```

**Size**: On mainnet epoch 507, this map contains **~11.2 million entries**.

**Counting UTXOs**:
```rust
// Navigate to UTxO State[0]
decoder.skip()?;  // [0] epoch
decoder.skip()?;  // [1] prev blocks
decoder.skip()?;  // [2] curr blocks

decoder.array()?;  // [3] Epoch State
decoder.skip()?;   // [3][0] Account State

decoder.array()?;  // [3][1] Ledger State
decoder.skip()?;   // [3][1][0] Cert State

decoder.array()?;  // [3][1][1] UTxO State

// [3][1][1][0] = UTXO map!
let utxo_count = match decoder.map()? {
    Some(len) => len,
    None => count_indefinite_map(&mut decoder)?,
};

println!("UTXO count: {}", utxo_count);
```

###### UTxO State[3]: Governance State

**Type**: `Array[4+]`

**Structure**:
```
[0] = Proposals (ARRAY):
      [0] = roots (ARRAY): [params, hard_fork, cc, constitution]
      [1] = proposals: Vec<ProposalState>
[1] = cc_state: ConstitutionalCommitteeState
[2] = constitution: Constitution
[3] = pparams: ProtocolParameters (current)
[4] = previous_pparams: ProtocolParameters
[5] = future_pparams: ProtocolParameters
[6] = DRep Pulsing State (ARRAY):
      [0] = Pulsing Snapshot
      [1] = Ratify State
      ...
```

**CIP-1694**: Conway governance structure introduced in epoch 505.

#### Element [3][2]: Snapshots

**Type**: Complex nested structure

**Description**: Historical snapshots used for rewards calculation.

**Parsing**: Typically skipped unless calculating historical rewards.

#### Element [3][3]: NonMyopic

**Type**: Various

**Description**: Non-myopic stake pool ranking data.

**Parsing**: Typically skipped.

### Element [4]: Rewards Update (Conditional)

**Type**: `Array[2]` (only if `has_rewards` is true)

**Condition**: Present if the snapshot includes reward calculation state. This is typically only available at epoch boundaries after rewards have been fully computed.

**Navigation Path**: `[4]` (top-level, after Epoch State)

**Structure**:
```
Rewards Update Array[2]:
  [0] = Pulsing State (ARRAY)
        [0] = status: u32  (1 = complete pulsing)
        [1] = Reward State (ARRAY)
              [0] = delta_treasury: i64
              [1] = delta_reserves: i64
              [2] = rewards: Map<StakeCredential, Set<Reward>>
              [3] = delta_fees: i64
  [1] = NonMyopic (duplicate of [3][3])
```

**Important Notes**:
- Most snapshots do NOT have rewards (`has_rewards = false`)
- When absent, elements `[4]`, `[5]`, and `[6]` must be skipped
- When present, the pulsing state status should be `1` (complete)
- Rewards map contains unclaimed rewards per stake credential
- Delta values show changes to treasury, reserves, and fees

**Example Usage**:
```rust
// After parsing Epoch State at [3]
d.skip()?;  // [3][2] Snapshots  
d.skip()?;  // [3][3] NonMyopic

// Check if snapshot has more elements (rewards present)
if has_rewards {
    d.array()?;  // [4] Rewards Update
    d.array()?;  // [4][0] Pulsing State
    
    let status: u32 = d.decode()?;
    assert_eq!(status, 1, "expected complete pulsing");
    
    d.array()?;  // [4][0][1] Reward State
    let delta_treasury: i64 = d.decode()?;
    let delta_reserves: i64 = d.decode()?;
    let rewards: BTreeMap<StakeCredential, Set<Reward>> = d.decode()?;
    let delta_fees: i64 = d.decode()?;
    
    d.skip()?;  // [4][1] NonMyopic
} else {
    // Skip unknown trailing elements
    d.skip()?;  // [4]
    d.skip()?;  // [5]  
    d.skip()?;  // [6]
}
```

**Note**: For most use cases (bootstrapping, querying state), rewards are not needed and `has_rewards` should be assumed `false`.

## Complete Navigation Example

Here's how to navigate to specific data:

### Get UTXO Count

```rust
use minicbor::decode::Decoder;

fn count_utxos(snapshot_path: &str) -> Result<u64, Error> {
    let bytes = std::fs::read(snapshot_path)?;
    let mut d = Decoder::new(&bytes);

    // Top-level array
    d.array()?;

    // Skip to Epoch State
    d.skip()?;  // [0] epoch
    d.skip()?;  // [1] prev blocks
    d.skip()?;  // [2] curr blocks

    // Enter Epoch State
    d.array()?;
    d.skip()?;  // [0] Account State

    // Enter Ledger State
    d.array()?;
    d.skip()?;  // [0] Cert State

    // Enter UTxO State
    d.array()?;

    // Count UTXO map entries
    let count = match d.map()? {
        Some(len) => len,
        None => {
            // Indefinite map - count manually
            let mut c = 0u64;
            loop {
                match d.datatype()? {
                    Type::Break => break,
                    _ => {
                        d.skip()?;  // key
                        d.skip()?;  // value
                        c += 1;
                    }
                }
            }
            c
        }
    };

    Ok(count)
}
```

### Get Treasury and Reserves

```rust
fn get_pots(snapshot_path: &str) -> Result<(u64, u64), Error> {
    let bytes = std::fs::read(snapshot_path)?;
    let mut d = Decoder::new(&bytes);

    d.array()?;
    d.skip()?;  // [0] epoch
    d.skip()?;  // [1] prev blocks
    d.skip()?;  // [2] curr blocks

    // Epoch State
    d.array()?;

    // Account State
    d.array()?;
    let treasury: i64 = d.decode()?;
    let reserves: i64 = d.decode()?;

    Ok((treasury as u64, reserves as u64))
}
```

### Get Protocol Parameters

```rust
fn get_protocol_params(snapshot_path: &str) -> Result<ProtocolParams, Error> {
    let bytes = std::fs::read(snapshot_path)?;
    let mut d = Decoder::new(&bytes);

    // Navigate to UTxO State[3] (governance state)
    d.array()?;
    d.skip()?; d.skip()?; d.skip()?;  // Skip to Epoch State

    d.array()?;
    d.skip()?;  // Account State

    d.array()?;
    d.skip()?;  // Cert State

    d.array()?;  // UTxO State
    d.skip()?;   // [0] utxo
    d.skip()?;   // [1] deposited
    d.skip()?;   // [2] fees

    // [3] = governance state
    d.array()?;
    d.skip()?;  // [0] proposals
    d.skip()?;  // [1] cc_state
    d.skip()?;  // [2] constitution

    // [3] = current protocol parameters
    let pparams: ProtocolParams = d.decode()?;

    Ok(pparams)
}
```

## Size Analysis

### Mainnet Snapshot (Epoch 507)

- **Total file size**: 2.55 GB
- **UTXO count**: 11,199,911
- **Top-level array elements**: 4+
- **Epoch State elements**: 4
- **Ledger State elements**: 2
- **UTxO State elements**: 6

### Parsing Performance

| Operation | Time | Memory |
|-----------|------|--------|
| Read epoch number (first 256KB) | <1 second | ~2.5 MB |
| Count UTXOs (full scan) | ~2 seconds | ~2.6 GB |
| Extract treasury/reserves | <1 second | ~50 MB |
| Full deserialization | Minutes | ~10+ GB |

## Era Compatibility

### Conway Era (Epoch 505+)

✅ **Fully Supported**

This structure description is accurate for Conway era snapshots.

**New in Conway**:
- CIP-1694 governance (DReps, proposals, constitutional committee)
- Inline datums and reference scripts in UTXOs
- Updated protocol parameters structure

### Pre-Conway Eras

❌ **Not Supported**

**Babbage (epochs ~358-504)**: Similar structure but missing governance fields.

**Alonzo and earlier**: Significantly different structure.

## Validation Checklist

When parsing a snapshot, validate:

1. ✅ Epoch >= 505 (Conway era)
2. ✅ Top-level array has at least 4 elements
3. ✅ Element [0] is u64 (epoch number)
4. ✅ Element [3] (Epoch State) is array[4]
5. ✅ Epoch State[1] (Ledger State) is array[2]
6. ✅ Ledger State[1] (UTxO State) is array[6+]
7. ✅ UTxO State[0] is a map (UTXO set)

## Implementation Notes

### Streaming vs Full Load

**Full Load** (current implementation):
- Reads entire file into memory (~2.6 GB for mainnet)
- Fast CBOR parsing with `minicbor`
- Simple navigation with `skip()` calls
- **Downside**: High memory usage

**Streaming** (future):
- Read file in chunks (e.g., 16 MB buffers)
- Maintain CBOR decoder state
- Navigate using byte offsets
- **Downside**: More complex, slower

For counting UTXOs, full load is acceptable since we need to scan the entire map anyway.

### Indefinite vs Definite Maps

CBOR supports both definite-length and indefinite-length maps:

```rust
match decoder.map()? {
    Some(len) => {
        // Definite: length is known upfront
        println!("Map has {} entries", len);
    }
    None => {
        // Indefinite: must count until break marker
        let mut count = 0;
        loop {
            match decoder.datatype()? {
                Type::Break => break,
                _ => {
                    decoder.skip()?;  // key
                    decoder.skip()?;  // value
                    count += 1;
                }
            }
        }
        println!("Map has {} entries", count);
    }
}
```

The UTXO map can be either type depending on how the snapshot was created.

## Real-World Example: Epoch 507 Snapshot

Based on actual parsing of mainnet epoch 507 snapshot (`134092758.670ca68c...cbor`):

```
File Size:              2.38 GB (2,553,095,916 bytes)
Epoch:                  507
Treasury:               1,528,154,947 ADA (1,528,154,947,846,618 lovelace)
Reserves:               7,816,251,180 ADA (7,816,251,180,544,575 lovelace)
Stake Pools:            3,095
DReps:                  278
Stake Accounts:         ~1.3M (to be measured)
UTXOs:                  11,199,911
Governance Proposals:   1
Tip Slot:               134,092,758
Estimated Height:       ~10,661,936
Boot Time:              ~6.5 seconds (with UTXO counting)
```

**Parsing Performance**:
- Epoch extraction: < 1 ms (reads first 256KB only)
- Boot data extraction: ~0.1 seconds (skip-based navigation)
- UTXO counting: ~1.1 seconds (11M UTXOs)
- Full boot: ~6.5 seconds (includes validation + counting)

**Key Observations**:
- Conway era (epoch >= 505) has governance structures
- Most complexity is in governance state (`[3][1][1][3]`)
- UTXO map is the largest structure (~10M+ entries)
- Snapshot does NOT have rewards (`has_rewards = false`)

## Future Work

### Phase 3: Targeted UTXO Lookup

Instead of counting all UTXOs, enable lookup by transaction input:

```rust
fn find_utxo(
    snapshot_path: &str,
    tx_hash: &[u8; 32],
    output_index: u64,
) -> Result<Option<TxOut>, Error> {
    // Navigate to UTXO map
    // Stream through entries
    // Return matching TxOut
}
```

**Benefit**: Constant memory, only parses matching entry.

### Phase 4: Full Ledger State Extraction

Extract all key metrics in one pass:

```rust
struct LedgerSnapshot {
    epoch: u64,
    utxo_count: u64,
    treasury: u64,
    reserves: u64,
    fees: u64,
    stake_accounts: u64,
    dreps: u64,
    stake_pools: u64,
    governance_proposals: u64,
}
```

## References

- [CIP-1694: Conway Governance](https://cips.cardano.org/cips/cip1694/)
- [Cardano Ledger Specs](https://github.com/IntersectMBO/cardano-ledger)
- [Amaru Implementation](https://github.com/pragma-org/amaru)
- [CBOR RFC 8949](https://www.rfc-editor.org/rfc/rfc8949.html)

## Change Log

- **2025-10-09**: Initial specification based on Conway era (epoch 507) snapshot analysis
- **Phase 1**: Epoch extraction and validation
- **Phase 2**: Corrected UTXO counting (11.2M UTXOs found)
