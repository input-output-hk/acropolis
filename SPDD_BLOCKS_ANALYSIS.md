# SPDD Stake Mismatch Issue - Comprehensive Analysis

## Executive Summary

The SPDD (SPO Distribution Distribution) stake mismatch issue appears to stem from **HOW THE BLOCKS FIELD IS POPULATED IN SNAPSHOTS**, not from a fundamental issue with stake calculations. The investigation reveals a critical gap between how snapshots are created at bootstrap vs. during live operation.

### Key Finding
- **SPDD (~142k ADA mismatch, 1077 pools affected)** uses **current stake** from `StakeAddressMap` (utxo_value + rewards)
- **EpochSnapshot.blocks field** must match EXACTLY what's used in reward calculations
- **Blocks are only populated for Mark snapshot** during bootstrap - Set and Go snapshots get empty block maps
- This discrepancy may cause reward calculations to fail silently or produce wrong results downstream

---

## 1. EpochSnapshot Structure

### Location
`/Users/algalon/RustroverProjects/acropolis/common/src/epoch_snapshot.rs`

### Structure Definition (lines 48-69)
```rust
pub struct EpochSnapshot {
    /// Epoch this snapshot is for (the one that has just ended)
    pub epoch: u64,

    /// Map of SPOs by operator ID with their delegation data
    pub spos: HashMap<PoolId, SnapshotSPO>,

    /// Total SPO (non-OBFT) blocks produced in this epoch
    pub blocks: usize,

    /// Pot balances at the time of this snapshot
    pub pots: Pots,

    /// Ordered registration changes that occurred during this epoch
    pub registration_changes: Vec<RegistrationChange>,
}
```

### SnapshotSPO Structure (lines 12-42)
```rust
pub struct SnapshotSPO {
    /// List of delegator stake addresses and their stake amounts
    pub delegators: Vec<(StakeAddress, Lovelace)>,

    /// Total stake delegated to this pool
    pub total_stake: Lovelace,

    /// Pool pledge amount
    pub pledge: Lovelace,

    /// Pool fixed cost
    pub fixed_cost: Lovelace,

    /// Pool margin (fee percentage)
    pub margin: Ratio,

    /// Number of blocks produced by this pool in this epoch
    pub blocks_produced: usize,

    /// Pool reward account
    pub reward_account: StakeAddress,

    /// Pool owners
    pub pool_owners: Vec<StakeAddress>,

    /// Is the reward account from two epochs ago registered at the time of this snapshot?
    pub two_previous_reward_account_is_registered: bool,
}
```

---

## 2. Where Blocks Field Is Set

### A. During Live Operation (EpochSnapshot::new - lines 74-222)

**Called with:**
```rust
pub fn new(
    epoch: u64,
    stake_addresses: &StakeAddressMap,
    spos: &imbl::OrdMap<PoolId, PoolRegistration>,
    spo_block_counts: &HashMap<PoolId, usize>,  // <- Block counts passed in
    pots: &Pots,
    blocks: usize,  // <- Total blocks passed directly
    registration_changes: Vec<RegistrationChange>,
    two_previous_snapshot: std::sync::Arc<EpochSnapshot>,
) -> Self
```

**Lines 98-99:**
```rust
let blocks_produced = spo_block_counts.get(spo_id).copied().unwrap_or(0);
```

**Lines 86-92:**
```rust
let mut snapshot = EpochSnapshot {
    epoch,
    pots: pots.clone(),
    blocks,  // <- Direct assignment from parameter
    registration_changes,
    ..EpochSnapshot::default()
};
```

### B. During Bootstrap (EpochSnapshot::from_raw - lines 240-260)

**Issue: No blocks parameter passed to this function!**

```rust
pub fn from_raw(
    epoch: u64,
    stake_map: HashMap<StakeCredential, i64>,
    delegation_map: HashMap<StakeCredential, PoolId>,
    pool_params_map: HashMap<PoolId, PoolRegistration>,
    block_counts: &HashMap<PoolId, usize>,  // <- Block counts available
    pots: Pots,
    network: NetworkId,
) -> Self {
    Self::from_raw_with_registration_check(
        epoch,
        stake_map,
        delegation_map,
        pool_params_map,
        block_counts,
        pots,
        network,
        None,    // <- No two_previous_snapshot
        None,    // <- No registered_credentials
    )
}
```

### C. Bootstrap Processing (lines 264-377)

**The crucial implementation in `from_raw_with_registration_check`:**

Lines 297-304:
```rust
// Second pass: build SPO entries and sum total blocks
let mut spos = HashMap::new();
let mut total_blocks: usize = 0;
for (pool_id, pool_reg) in pool_params_map {
    let delegators = delegations_by_pool.remove(&pool_id).unwrap_or_default();
    let total_stake = stake_by_pool.get(&pool_id).copied().unwrap_or(0);
    let blocks_produced = block_counts.get(&pool_id).copied().unwrap_or(0);
    total_blocks += blocks_produced;  // <- Accumulates per-pool blocks
```

Lines 370-376:
```rust
EpochSnapshot {
    epoch,
    spos,
    blocks: total_blocks,  // <- Sets from sum of all pool blocks
    pots,
    registration_changes: Vec::new(),
}
```

---

## 3. Mark/Set/Go Snapshot Block Assignment

### Location
`/Users/algalon/RustroverProjects/acropolis/common/src/snapshot/mark_set_go.rs`

### Raw Snapshots Container (lines 162-174)

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawSnapshotsContainer {
    /// Mark snapshot (raw CBOR data)
    pub mark: RawSnapshot,
    /// Set snapshot (raw CBOR data)
    pub set: RawSnapshot,
    /// Go snapshot (raw CBOR data)
    pub go: RawSnapshot,
    /// Fee snapshot (feeSS) - fees at epoch boundary
    pub fee_ss: u64,
}
```

### The Critical Issue: Block Assignment During Bootstrap (lines 205-241)

```rust
pub fn into_snapshots_container(
    self,
    epoch: u64,
    blocks_previous_epoch: &HashMap<PoolId, usize>,
    blocks_current_epoch: &HashMap<PoolId, usize>,
    pots: Pots,
    network: NetworkId,
) -> SnapshotsContainer {
    let empty_blocks = HashMap::new();

    // Epoch assignments - snapshots are from epoch boundaries BEFORE the current epoch:
    // - Mark = epoch - 1 (newest, has blocks from previous epoch)
    // - Set = epoch - 2
    // - Go = epoch - 3 (oldest, used for staking in rewards calculation)
    
    let mark = self.mark.into_snapshot(
        epoch.saturating_sub(1),
        blocks_previous_epoch,    // <- Mark gets blocks_previous_epoch
        pots,
        network.clone(),
    );

    let set = self.set.into_snapshot(
        epoch.saturating_sub(2),
        blocks_current_epoch,     // <- Set gets blocks_current_epoch
        Pots::default(),
        network.clone(),
    );

    let go = self.go.into_snapshot(
        epoch.saturating_sub(3),
        &empty_blocks,            // <-- GO GETS EMPTY BLOCKS! CRITICAL ISSUE
        Pots::default(),
        network,
    );

    SnapshotsContainer { mark, set, go }
}
```

### The Comments Explain The Logic (lines 181-195)

```
The CBOR file stores snapshots in order: [Mark, Set, Go, Fee]

IMPORTANT: The snapshots in the CBOR are taken at epoch BOUNDARIES, not at the
current epoch. For a New Epoch State snapshot captured during epoch N:
- Mark contains stake distribution from end of epoch N-1 (snapshot taken at N-1→N boundary)
- Set contains stake distribution from end of epoch N-2 (snapshot taken at N-2→N-1 boundary)
- Go contains stake distribution from end of epoch N-3 (snapshot taken at N-3→N-2 boundary)

In Cardano terminology for rewards calculation:
- Mark = newest snapshot (epoch N-1) - used as "performance" snapshot (blocks_produced)
- Set = middle snapshot (epoch N-2)
- Go = oldest snapshot (epoch N-3) - used as "staking" snapshot for rewards

Block count assignments:
- Mark (epoch N-1): Uses blocks_previous_epoch (blocks produced in epoch N-1)
- Set (epoch N-2): Uses blocks_current_epoch (actually from epoch before that)
- Go (epoch N-3): No block data available during bootstrap
```

---

## 4. How Blocks Are Used in Reward Calculations

### Location
`/Users/algalon/RustroverProjects/acropolis/modules/accounts_state/src/rewards.rs`

### Total Blocks Accessed (lines 94-98)

```rust
// If no blocks produced in previous epoch, don't do anything
let total_blocks = performance.blocks;  // <- Uses blocks from performance snapshot
if total_blocks == 0 {
    return Ok(result);
}
```

### Apparent Performance Calculation (lines 375-394)

```rust
// mkApparentPerformance d σa n N:
// - If d >= 0.8: returns 1 (full decentralization)
// - Otherwise: β/σa where β = n/max(1,N)
let decentralisation = &params.protocol_params.decentralisation_param;
let pool_performance = if decentralisation >= &RationalNumber::new(8, 10) {
    BigDecimal::one()
} else {
    // σa = pool_stake / total_active_stake (NOT total_supply!)
    let relative_active_stake = &pool_stake / total_active_stake;
    // β = blocks_produced / total_blocks
    let relative_blocks = BigDecimal::from(blocks_produced)
        / BigDecimal::from(total_blocks as u64);  // <-- Uses total_blocks

    debug!(blocks_produced, %relative_blocks, %pool_stake, %relative_active_stake,
           "Pool performance calc (mkApparentPerformance):");
    &relative_blocks / &relative_active_stake
};
```

**Critical Formula (Shelley Spec Figure 46):**
```
mkApparentPerformance d σ n N = β/σ  if d < 0.8, else 1
where β = n/max(1,N)
```

If `N` (total_blocks) is 0, this formula breaks down with division by zero.

---

## 5. SPDD Generation and Stake Snapshot Usage

### Location
`/Users/algalon/RustroverProjects/acropolis/common/src/stake_addresses.rs` (lines 284-320)

### SPDD Generation Code

```rust
pub fn generate_spdd(&self) -> BTreeMap<PoolId, DelegatedStake> {
    // Shareable Dashmap with referenced keys
    let spo_stakes = DashMap::<PoolId, DelegatedStake>::new();

    // Total stake across all addresses in parallel, first collecting into a vector
    // Collect the SPO keys and UTXO, reward values
    let sas_data: Vec<(PoolId, (u64, u64))> = self
        .inner
        .values()
        .filter_map(|sas| {
            sas.delegated_spo.as_ref().map(|spo| (*spo, (sas.utxo_value, sas.rewards)))
        })
        .collect();

    // Parallel sum all the stakes into the spo_stake map
    sas_data
        .par_iter() // Rayon multi-threaded iterator
        .for_each(|(spo, (utxo_value, rewards))| {
            let total_stake = *utxo_value + *rewards;  // <-- Active stake definition
            spo_stakes
                .entry(*spo)
                .and_modify(|v| {
                    v.active += total_stake;
                    v.active_delegators_count += 1;
                    v.live += total_stake;
                })
                .or_insert(DelegatedStake {
                    active: total_stake,
                    active_delegators_count: 1,
                    live: total_stake,
                });
        });

    // Collect into a plain BTreeMap, so that it is ordered on output
    spo_stakes.iter().map(|entry| (*entry.key(), *entry.value())).collect()
}
```

**Key Insight:** SPDD uses **current stake address state** (utxo_value + rewards), NOT snapshot data.

---

## 6. The Root Cause Analysis

### Problem Scenario

1. **At Bootstrap (Epoch N):**
   - Streaming snapshot parser reads CBOR bootstrap file
   - Parses mark, set, go snapshots with their ORIGINAL stake distributions
   - Parses `blocks_previous_epoch` and `blocks_current_epoch` from CBOR
   - Creates 3 snapshots:
     - **Mark (N-1):** gets `blocks_previous_epoch`
     - **Set (N-2):** gets `blocks_current_epoch`
     - **Go (N-3):** gets `empty_blocks` ← **CRITICAL: No blocks!**

2. **At Next Epoch Boundary:**
   - SPDD is generated from **current StakeAddressMap** (line 288 in accounts_state.rs)
   - Snapshots are rotated
   - `Mark` from previous epoch becomes `Set`
   - `Set` from previous epoch becomes `Go`
   - **New Mark** is created from current state

3. **Where Mismatch Occurs:**
   - If rewards calculation tries to use `Go.blocks` (which is 0)
   - Performance calculations fail or return wrong values
   - This cascades to SPDD since SPDD uses live stake calculations
   - **142k ADA difference × 1077 pools** suggests systematic undercounting

---

## 7. Streaming Snapshot Block Parsing

### Location
`/Users/algalon/RustroverProjects/acropolis/common/src/snapshot/streaming_snapshot.rs`

### Block Parsing Implementation (lines 1708-1823)

```rust
fn parse_blocks_with_epoch(
    decoder: &mut Decoder,
    epoch: u64,
) -> Result<Vec<PoolBlockProduction>> {
    // Blocks are typically encoded as an array or map
    match decoder.datatype().context("Failed to read blocks datatype")? {
        Type::Array | Type::ArrayIndef => {
            let len = decoder.array().context("Failed to parse blocks array")?;
            let blocks = Vec::new();

            // Handle definite-length array
            if let Some(block_count) = len {
                for _i in 0..block_count {
                    // Each block might be encoded as an array or map
                    // For now, skip individual blocks since we don't know the exact format
                    // This is a placeholder - the actual format needs to be determined from real data
                    decoder.skip().context("Failed to skip block entry")?;
                }
            } else {
                // Indefinite-length array
                info!("Processing indefinite-length blocks array");
                // ... Handle indefinite-length array
            }

            Ok(blocks)  // <- Returns Vec::new() with no blocks populated!
        }
        Type::Map | Type::MapIndef => {
            // Blocks are stored as a map: PoolID -> block_count (u8)
            let len = decoder.map().context("Failed to parse blocks map")?;

            let mut block_productions = Vec::new();

            // Parse map content
            if let Some(entry_count) = len {
                for _i in 0..entry_count {
                    // Parse pool ID -> block count
                    match Self::parse_single_block_production_entry(decoder, epoch) {
                        Ok(production) => {
                            block_productions.push(production);
                        }
                        Err(_) => {
                            // Skip failed entries
                            decoder.skip().context("Failed to skip map key")?;
                            decoder.skip().context("Failed to skip map value")?;
                        }
                    }
                }
            }
            // ... Indefinite map handling

            Ok(block_productions)  // <- Returns populated block_productions
        }
        // ... Other type handlers
    }
}
```

**CRITICAL ISSUE IN COMMENT (lines 1722-1723):**
```
For now, skip individual blocks since we don't know the exact format
This is a placeholder - the actual format needs to be determined from real data
```

### Where Blocks Are Called (lines 835-840)

```rust
// Parse blocks_previous_epoch [1] and blocks_current_epoch [2]
let blocks_previous_epoch =
    Self::parse_blocks_with_epoch(&mut decoder, epoch.saturating_sub(1))
        .context("Failed to parse blocks_previous_epoch")?;
let blocks_current_epoch = Self::parse_blocks_with_epoch(&mut decoder, epoch)
    .context("Failed to parse blocks_current_epoch")?;
```

---

## 8. Identified Issues and Mismatch Mechanisms

### Issue 1: Empty Blocks for Go Snapshot
- **Severity:** HIGH
- **Location:** mark_set_go.rs lines 233-238
- **Problem:** Go snapshot always receives `empty_blocks` HashMap during bootstrap
- **Impact:** Go snapshot has `blocks: 0`, which may cause reward calculations to skip or fail
- **Consequence:** Cascading effects on stake distribution and SPDD calculations

### Issue 2: Array-Type Block Data Not Parsed
- **Severity:** MEDIUM-HIGH
- **Location:** streaming_snapshot.rs lines 1716-1746
- **Problem:** When blocks are encoded as Array type, the function returns empty Vec without parsing
- **Impact:** If bootstrap CBOR has Array-encoded blocks, they're silently dropped
- **Consequence:** Block counts may be systematically undercounted

### Issue 3: Missing Total Blocks Field
- **Severity:** MEDIUM
- **Location:** streaming_snapshot.rs parse_blocks_with_epoch
- **Problem:** Function returns `Vec<PoolBlockProduction>` but never calculates total blocks
- **Impact:** Individual pool blocks parsed, but total for snapshot not calculated
- **Consequence:** Into_snapshot() calls need to recalculate total from Vec

### Issue 4: No Validation of Blocks After Parsing
- **Severity:** MEDIUM
- **Location:** mark_set_go.rs into_snapshots_container
- **Problem:** No validation that blocks were actually parsed vs. defaulted to empty
- **Impact:** Silent failures - empty blocks accepted without warning
- **Consequence:** Reward calculations proceed with 0 blocks without detection

### Issue 5: Snapshot Rotation Bug
- **Severity:** LOW-MEDIUM
- **Location:** Across multiple files
- **Problem:** When snapshots rotate, old Mark becomes new Set, but block history not preserved correctly
- **Impact:** Historic block data from earlier epochs may be lost during rotation
- **Consequence:** Mismatch between expected and actual block counts

---

## 9. Summary Table: Blocks Field Population

| Snapshot | Created By | Block Source | Status | Issue |
|----------|-----------|--------------|--------|-------|
| Mark (live) | accounts_state.rs line 282 | epoch_activity blocks | Correct | None |
| Set (live) | Snapshot rotation | Previous Mark blocks | Correct | None |
| Go (live) | Snapshot rotation | Previous Set blocks | Correct | None |
| Mark (bootstrap) | mark_set_go.rs line 219 | blocks_previous_epoch param | May be empty | ✓ If CBOR has Array format |
| Set (bootstrap) | mark_set_go.rs line 226 | blocks_current_epoch param | May be empty | ✓ If CBOR has Array format |
| Go (bootstrap) | mark_set_go.rs line 233 | `&empty_blocks` | Always 0 | ✓ ALWAYS EMPTY |

---

## 10. Recommendations for Investigation

### Immediate Debugging Steps

1. **Check Streaming Snapshot Parse Logs:**
   - Enable `parse_blocks_with_epoch` debug logging
   - Verify which datatype (Array vs Map) blocks are encoded as
   - Confirm whether Array blocks are being silently dropped

2. **Validate Block Counts After Bootstrap:**
   - Print `Mark.blocks`, `Set.blocks`, `Go.blocks` after bootstrap
   - Compare against expected values from CBOR inspection
   - Check if Go.blocks is always 0

3. **Test Reward Calculation with Zero Blocks:**
   - Trace through mkApparentPerformance when total_blocks = 0
   - Check if division-by-zero is being handled
   - Verify if rewards are being skipped

4. **Compare Bootstrap vs Live SPDD:**
   - Generate SPDD immediately after bootstrap
   - Compare with SPDD from live epoch N
   - Calculate exact ADA difference per pool

### Long-term Fixes

1. **Fix Go Snapshot Block Assignment:**
   - Store historical block data for Go snapshot
   - Alternative: Calculate from available block production data
   - Worst case: Document that rewards are incomplete for first N epochs

2. **Complete Block Parsing:**
   - Implement proper Array-type block parsing in streaming_snapshot.rs
   - Add validation that blocks were successfully parsed
   - Add warnings if blocks default to empty

3. **Add Comprehensive Validation:**
   - Validate snapshot.blocks against sum of snapshot.spos[*].blocks_produced
   - Compare bootstrap blocks to live blocks in subsequent epochs
   - Add metrics for block count integrity

4. **Snapshot Rotation Audit:**
   - Document exact block data preservation during rotation
   - Add logging for block changes between epochs
   - Validate consistency across epoch boundaries

---

## 11. Key Code References

| Topic | File | Lines | Key Content |
|-------|------|-------|------------|
| EpochSnapshot structure | epoch_snapshot.rs | 48-69 | Field definitions |
| SPDD generation | stake_addresses.rs | 284-320 | Active stake calculation |
| Reward calculation | rewards.rs | 375-394 | mkApparentPerformance |
| Bootstrap snapshots | mark_set_go.rs | 205-241 | Block assignment |
| Block parsing | streaming_snapshot.rs | 1708-1823 | Array/Map handling |
| Live snapshot creation | epoch_snapshot.rs | 74-222 | EpochSnapshot::new |
| SPDD publication | accounts_state.rs | 288-292 | generate_spdd() call |

---

## Conclusion

**The SPDD stake mismatch appears fundamentally linked to how the `blocks` field is populated in snapshots, particularly:**

1. **Go snapshot has no blocks** (always empty after bootstrap)
2. **Array-encoded blocks may be silently dropped** during parsing
3. **No validation** occurs to detect if blocks failed to parse

This causes cascading errors in:
- Reward calculations (uses blocks for apparent performance)
- Stake calculations (dependencies from reward calculations)
- SPDD generation (uses stake from accounts affected by reward calcs)

The ~142k ADA mismatch across 1077 pools suggests a **systematic undercounting of rewards** due to zero or missing block data, which then affects the live stake calculations used by SPDD.
