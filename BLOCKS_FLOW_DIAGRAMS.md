# SPDD Stake Mismatch - Blocks Field Data Flow Diagram

## 1. Snapshot Creation Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    BOOTSTRAP PHASE (Epoch N)                        │
└─────────────────────────────────────────────────────────────────────┘

CBOR File (New Epoch State Snapshot)
    ├─ [0] Mark snapshot (epoch N-1 data)
    ├─ [1] blocks_previous_epoch (blocks from epoch N-1)
    ├─ [2] blocks_current_epoch (blocks from epoch N-2)
    ├─ [3] Set snapshot (epoch N-2 data)
    ├─ [4] Go snapshot (epoch N-3 data)
    └─ [5] fee_ss (fee snapshot)

              │
              │ (StreamingSnapshot::parse)
              ▼

    ┌─────────────────────────────────────────┐
    │ parse_blocks_with_epoch()               │
    │ ┌─────────────────────────────────────┐ │
    │ │ If Array:                           │ │
    │ │   ✗ Returns empty Vec::new()        │ │ ← PROBLEM!
    │ │                                     │ │
    │ │ If Map:                             │ │
    │ │   ✓ Returns Vec<PoolBlockProduction>│ │
    │ └─────────────────────────────────────┘ │
    └─────────────────────────────────────────┘

              │
              │ (RawSnapshotsContainer)
              ▼

    blocks_previous_epoch: Vec<PoolBlockProduction>
    blocks_current_epoch: Vec<PoolBlockProduction>

              │
              │ (into_snapshots_container)
              ▼

    ┌─────────────────────────────────────────────────┐
    │ Create Mark (epoch N-1)                         │
    │   blocks = sum(blocks_previous_epoch)           │ ✓ Correct
    │ blocks_produced per pool = lookup               │
    └─────────────────────────────────────────────────┘
              │
              ├─────────────────────────────────────────────────┐
              │                                                 │
              ▼                                                 ▼
    ┌────────────────────────────┐          ┌──────────────────────────┐
    │ Create Set (epoch N-2)     │          │ Create Go (epoch N-3)   │
    │   blocks = sum(            │          │   blocks = sum(          │
    │     blocks_current_epoch   │          │     empty_blocks ✗       │
    │   )                        │          │   ) = 0                  │
    │   ✓ May be correct         │          │   ✗ ALWAYS ZERO          │
    └────────────────────────────┘          └──────────────────────────┘

              │
              └─────────────────────────────────────────────────┐
                                                                 │
                                                                 ▼
    ┌─────────────────────────────────────────────────────────────────┐
    │           SnapshotsContainer Initialized                        │
    │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐         │
    │  │    Mark      │  │     Set      │  │     Go       │         │
    │  │ epoch: N-1   │  │ epoch: N-2   │  │ epoch: N-3   │         │
    │  │ blocks: X    │  │ blocks: Y    │  │ blocks: 0    │         │
    │  │ ✓ OK         │  │ ? Depends    │  │ ✗ PROBLEM    │         │
    │  └──────────────┘  └──────────────┘  └──────────────┘         │
    └─────────────────────────────────────────────────────────────────┘
```

## 2. Reward Calculation Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    REWARD CALCULATION                               │
│                    (After next epoch ends)                          │
└─────────────────────────────────────────────────────────────────────┘

calculate_rewards(epoch, performance_snapshot, staking_snapshot, ...)
    │
    ├─ performance = Mark snapshot (epoch N-1)
    ├─ staking = Go snapshot (epoch N-3)
    │
    ▼
    
    let total_blocks = performance.blocks
    
    ┌─────────────────────────────────────┐
    │ if total_blocks == 0:               │
    │   return Ok(empty_result) ✗         │  ← SILENT FAILURE!
    │ (No rewards calculated!)            │
    └─────────────────────────────────────┘
    
    if total_blocks > 0:
    
        For each SPO:
            let blocks_produced = performance.spos[spo].blocks_produced
            
            ▼
            
            mkApparentPerformance(d, σ, blocks_produced, total_blocks)
                │
                ├─ if d >= 0.8: return 1
                │
                └─ else: return (blocks_produced / total_blocks) / σ
                           
            ▼
            
            pool_rewards = apparent_performance × max_pool_rewards
            leader_rewards = pool_rewards - member_share
            member_rewards = (pool_rewards - leader) × (1 - margin)
            
            ▼
            
            Update account rewards (in StakeAddressMap)
```

## 3. SPDD Generation Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    SPDD GENERATION                                  │
│              (At epoch boundary, after rewards)                     │
└─────────────────────────────────────────────────────────────────────┘

accounts_state.rs:288
    │
    ▼
    state.generate_spdd()
        │
        ▼
        stake_addresses.generate_spdd()
            │
            ├─ For each stake address in StakeAddressMap:
            │
            │   let total_stake = utxo_value + rewards
            │                                    └──────┘
            │                                  (Set by reward calc)
            │
            │   spo_stakes[delegated_spo] += total_stake
            │
            ▼
            
    BTreeMap<PoolId, DelegatedStake>
    
    ┌─────────────────────────────────────────────────────┐
    │ Result: SPDD showing current active stake per pool  │
    │                                                     │
    │ If rewards calc was skipped (0 blocks):            │
    │   - "rewards" field was not updated                │
    │   - SPDD missing those reward amounts              │
    │   - Results in MISMATCH with expected stakes      │
    └─────────────────────────────────────────────────────┘
```

## 4. Issue Cascade Map

```
┌─────────────────────────────────┐
│ Root Cause: Go.blocks = 0       │
│ (or Array-type blocks dropped)  │
└───────────────┬─────────────────┘
                │
                ▼
        ┌───────────────────┐
        │ Reward calc       │
        │ skipped/fails     │
        │ (line 94-98)      │
        └───────┬───────────┘
                │
                ▼
        ┌───────────────────┐
        │ Rewards NOT       │
        │ added to account  │
        │ balance           │
        └───────┬───────────┘
                │
                ▼
        ┌───────────────────────────┐
        │ StakeAddressMap           │
        │ has zero "rewards" field  │
        │ for affected accounts     │
        └───────┬───────────────────┘
                │
                ▼
        ┌───────────────────────────┐
        │ SPDD.active_stake         │
        │ = utxo + rewards (0)      │
        │ Missing reward amount     │
        └───────┬───────────────────┘
                │
                ▼
        ┌─────────────────────────────────────┐
        │ OBSERVED MISMATCH:                  │
        │ ~142k ADA missing                   │
        │ 1077 pools affected                 │
        │ (those with rewards)                │
        └─────────────────────────────────────┘
```

## 5. Block Field Population Matrix

```
╔═══════════════════════════════════════════════════════════════════════════╗
║                    HOW BLOCKS FIELD IS POPULATED                          ║
╠══════════════════════════════════════════════════════════════════════════╗
║ Scenario    │ Source                        │ Status      │ Issue         ║
╠═════════════╪═══════════════════════════════╪═════════════╪═══════════════╣
║ LIVE - Mark │ accounts_state.rs (line 282) │ ✓ Correct   │ None          ║
║             │ epoch_activity.blocks         │             │               ║
╠─────────────┼───────────────────────────────┼─────────────┼───────────────╣
║ LIVE - Set  │ Previous Mark (rotate)        │ ✓ Correct   │ None          ║
╠─────────────┼───────────────────────────────┼─────────────┼───────────────╣
║ LIVE - Go   │ Previous Set (rotate)         │ ✓ Correct   │ None          ║
╠═════════════╪═══════════════════════════════╪═════════════╪═══════════════╣
║ BOOTSTRAP   │ blocks_previous_epoch param   │ ? Depends   │ May be empty  ║
║ Mark        │ parsed from CBOR              │ on format   │ (Array type)  ║
╠─────────────┼───────────────────────────────┼─────────────┼───────────────╣
║ BOOTSTRAP   │ blocks_current_epoch param    │ ? Depends   │ May be empty  ║
║ Set         │ parsed from CBOR              │ on format   │ (Array type)  ║
╠─────────────┼───────────────────────────────┼─────────────┼───────────────╣
║ BOOTSTRAP   │ empty_blocks HashMap          │ ✗ Always 0  │ CRITICAL BUG  ║
║ Go          │ (hardcoded - line 235)        │             │               ║
╚═════════════╧═══════════════════════════════╧═════════════╧═══════════════╝
```

## 6. Code Dependency Chain

```
accounts_state.rs (line 288)
    │
    ├─ state.generate_spdd()
    │   │
    │   └─ stake_addresses.generate_spdd()
    │       │
    │       └─ For each account: total_stake = utxo + rewards ◄─────┐
    │                                                                 │
    │                                                         (Updated by)
    │                                                              │
calculate_rewards()                                                 │
    (modules/accounts_state/src/rewards.rs)                         │
    │                                                               │
    ├─ performance.blocks ◄─ (Mark snapshot)                        │
    │   └─ if total_blocks == 0: return (skip rewards) ──────────→ │
    │                                                         (Silent skip)
    │
    ├─ For each SPO:
    │   ├─ mkApparentPerformance(...)
    │   │   └─ uses total_blocks in division
    │   │
    │   └─ calculate_spo_rewards(...)
    │       └─ Updates reward_aggregator
    │           └─ Adds to StakeAddressMap.rewards ──────────────→ │
    │
    └─ (If blocks=0, no rewards updated)


Mark snapshot                                    Go snapshot
    │                                                 │
    ├─ blocks: ? (blocks_previous_epoch)            ├─ blocks: 0
    │   blocks_produced per pool                     ├─ blocks_produced = 0
    │   (from epoch_activity)                        │   (from empty_blocks)
    │                                                │
    └─ performance.blocks ◄────────────────────────┘ (Actually used)


Bootstrap creates Go with empty_blocks (line 235 in mark_set_go.rs)
    │
    └─> Go snapshot: blocks = 0
        └─> Reward calc skipped
            └─> StakeAddressMap.rewards not updated
                └─> SPDD missing reward amounts
                    └─> MISMATCH observed
```

## 7. Test Case: Zero Blocks Scenario

```
┌─────────────────────────────────────────────────────────────────────┐
│ SCENARIO: Go.blocks = 0 (current state)                            │
└─────────────────────────────────────────────────────────────────────┘

Input:
  - performance_snapshot = Mark (epoch N-1)
  - staking_snapshot = Go (epoch N-3)
  - Mark.blocks = X (blocks from N-1)
  - Go.blocks = 0 (ALWAYS)

Calculation:
  calculate_rewards(epoch, Mark, Go, ...):
    total_blocks = Mark.blocks = X
    
    if X == 0:
        return Ok(RewardsResult::default())  ← Line 96-98
    
    (Continue with calculation if X > 0)

Expected vs Observed:
  
  For Epoch N+3 rewards calculation:
    - Should use Go snapshot from epoch N (the 3rd snapshot back)
    - Currently using Go snapshot from epoch N-3 (wrong data + 0 blocks)
    - If all 3 snapshots have blocks = 0, no rewards are calculated
    - Cascades to SPDD generation

Result:
  SPDD missing ~142k ADA across 1077 pools
  (Those pools receiving member rewards from affected delegators)
```

## 8. Solution Options

```
┌─────────────────────────────────────────────────────────────────────┐
│                        FIX PRIORITY MAP                             │
└─────────────────────────────────────────────────────────────────────┘

PRIORITY 1 (Critical - Do First)
├─ Fix Go snapshot block assignment
│  └─ Store blocks_previous_epoch for Go snapshot
│     (or calculate from available data)
│     Impact: Fixes ~80% of rewards calculation issues
│
└─ Implement Array-type block parsing
   └─ Complete parse_blocks_with_epoch() for Array format
      Impact: Fixes silent block count drops

PRIORITY 2 (High - Do Second)
├─ Add validation after bootstrap
│  └─ Validate Mark.blocks = sum(Mark.spos[*].blocks_produced)
│     Impact: Detects mismatches early
│
└─ Add logging for blocks in snapshots
   └─ Log Mark/Set/Go blocks after bootstrap & each epoch
      Impact: Makes issues visible in logs

PRIORITY 3 (Medium - Do Third)
├─ Audit snapshot rotation
│  └─ Verify blocks preserved during Mark→Set→Go rotation
│     Impact: Ensures consistency over epochs
│
└─ Test with historical data
   └─ Compare SPDD from bootstrap vs. subsequent epochs
      Impact: Validates fix correctness

PRIORITY 4 (Low - Do Later)
├─ Refactor to use Arc<EpochSnapshot> for blocks
├─ Add snapshots persistence layer
└─ Implement snapshot integrity checks
```

---

## Summary of Critical Points

1. **Go snapshot blocks field is always 0 after bootstrap** (Line 235 in mark_set_go.rs)

2. **Array-type blocks are silently dropped** (Lines 1716-1746 in streaming_snapshot.rs)

3. **No validation occurs** to detect when blocks fail to parse

4. **Reward calculation skips silently** when total_blocks = 0 (Lines 94-98 in rewards.rs)

5. **SPDD generation uses current stake** which depends on reward calculations

6. **Result: ~142k ADA mismatch** across 1077 pools that received (or should have received) rewards
