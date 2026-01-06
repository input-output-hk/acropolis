# SPDD Stake Mismatch - Quick Reference Guide

## The Problem in One Sentence
**Go snapshot has 0 blocks (or blocks parsing fails) → reward calculations skip → StakeAddressMap.rewards not updated → SPDD missing ~142k ADA across 1077 pools**

---

## Critical File Locations

| Issue | File | Lines | Problem |
|-------|------|-------|---------|
| Go block assignment | `common/src/snapshot/mark_set_go.rs` | 233-238 | `&empty_blocks` hardcoded |
| Array block parsing | `common/src/snapshot/streaming_snapshot.rs` | 1716-1746 | Returns empty Vec |
| Silent reward skip | `modules/accounts_state/src/rewards.rs` | 94-98 | `if total_blocks == 0: return` |
| Snapshot structure | `common/src/epoch_snapshot.rs` | 48-69 | `blocks: usize` field |
| SPDD generation | `common/src/stake_addresses.rs` | 284-320 | Uses `utxo + rewards` |

---

## The Data Flow

```
CBOR Bootstrap File
    ↓
parse_blocks_with_epoch() ← May drop Array-type blocks
    ↓
RawSnapshotsContainer
    ↓
into_snapshots_container()
    ├─ Mark.blocks = blocks_previous_epoch ✓
    ├─ Set.blocks = blocks_current_epoch ✓
    └─ Go.blocks = empty_blocks = 0 ✗
    ↓
SnapshotsContainer
    ↓
calculate_rewards(performance=Mark, staking=Go)
    ├─ total_blocks = performance.blocks
    └─ if total_blocks == 0: return (skip all rewards) ✗
    ↓
StakeAddressMap.rewards ← Not updated!
    ↓
generate_spdd()
    └─ stake = utxo + rewards (0 if skip) ✗
    ↓
SPDD Mismatch ~142k ADA
```

---

## Immediate Debugging Checklist

- [ ] Check CBOR block encoding format (Array vs Map)?
- [ ] Log Mark/Set/Go.blocks after bootstrap initialization
- [ ] Verify Go.blocks is always 0
- [ ] Search logs for "if total_blocks == 0"
- [ ] Compare SPDD right after bootstrap vs. epoch N
- [ ] Calculate which pools are missing the most ADA
- [ ] Check if missing ADA = expected rewards

---

## Root Causes (Priority Order)

### 1. Go Snapshot = 0 Blocks (CRITICAL)
**File:** `common/src/snapshot/mark_set_go.rs:235`
```rust
let go = self.go.into_snapshot(
    epoch.saturating_sub(3),
    &empty_blocks,  // ← ALWAYS EMPTY!
    Pots::default(),
    network,
);
```
**Fix:** Store blocks data for Go snapshot (need blocks_epoch_minus_3)

### 2. Array-Type Blocks Dropped (HIGH)
**File:** `common/src/snapshot/streaming_snapshot.rs:1716`
```rust
Type::Array | Type::ArrayIndef => {
    let blocks = Vec::new();  // ← Returns empty!
    // ... skip and return blocks (empty)
    Ok(blocks)
}
```
**Fix:** Implement proper Array parsing like Map parsing

### 3. Silent Reward Calculation Skip (HIGH)
**File:** `modules/accounts_state/src/rewards.rs:94-98`
```rust
let total_blocks = performance.blocks;
if total_blocks == 0 {
    return Ok(result);  // ← No error, just skip silently!
}
```
**Fix:** Add warning/error logging or handle gracefully

### 4. No Validation (MEDIUM)
**File:** Multiple locations
**Problem:** Bootstrap succeeds even if blocks are missing
**Fix:** Add validation: `assert!(snapshot.blocks == sum(snapshot.spos[*].blocks_produced))`

---

## Mismatch Pattern

**Affected Pools:** ~1077 (those with rewards)
**Missing ADA:** ~142,000 (across all)
**Pattern:** 
- Pools with blocks_produced > 0 in Mark snapshot
- Should receive rewards in reward calculation
- Go.blocks = 0 causes entire reward calc to skip
- Result: rewards not added to delegators' StakeAddressMap
- SPDD shows original utxo without accumulated rewards

---

## Diagnostic Commands

```bash
# Show snapshot block counts (add to logging)
info!(mark.blocks, set.blocks, go.blocks, "Snapshot blocks after bootstrap");

# Search for block parsing issues
grep -n "Array\|parse_blocks" common/src/snapshot/streaming_snapshot.rs

# Find where rewards are used
grep -n "\.rewards" common/src/stake_addresses.rs

# Check reward calculation paths
grep -n "total_blocks\|if.*== 0" modules/accounts_state/src/rewards.rs
```

---

## Expected vs Observed

### Expected Behavior
- Mark.blocks = sum of blocks from epoch N-1 (from blocks_previous_epoch)
- Set.blocks = sum of blocks from epoch N-2 (from blocks_current_epoch)
- Go.blocks = sum of blocks from epoch N-3 (from blocks_epoch_minus_3, **missing**)
- Reward calc uses Mark for performance, staking snapshot for stake
- Rewards added to StakeAddressMap
- SPDD = active stake including rewards

### Observed Behavior
- Mark.blocks = ? (correct if blocks_previous_epoch parsed)
- Set.blocks = ? (correct if blocks_current_epoch parsed)
- Go.blocks = 0 (always, hardcoded)
- Reward calc skips when Go.blocks used or Mark.blocks used
- Rewards NOT added to StakeAddressMap
- SPDD = active stake WITHOUT rewards = mismatch

---

## References

Full analysis: `/Users/algalon/RustroverProjects/acropolis/SPDD_BLOCKS_ANALYSIS.md`
Flow diagrams: `/Users/algalon/RustroverProjects/acropolis/BLOCKS_FLOW_DIAGRAMS.md`

---

## Key Insight

**The 142k ADA mismatch isn't "missing stake" - it's missing REWARDS.**

When Go.blocks = 0:
- Reward calculation is skipped
- No rewards added to accounts
- SPDD shows utxo+0 instead of utxo+rewards
- Difference = rewards that should have been distributed

This cascades because:
1. Some accounts' rewards never get added
2. SPDD shows lower active stake than it should
3. 1077 pools affected (those with delegators who earned rewards)
4. ~142k ADA total difference (sum of missing rewards)

