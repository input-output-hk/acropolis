# Rewards Investigation Summary

## Current Status (Epoch 509+)

| Category | Observed | Expected | Difference |
|----------|----------|----------|------------|
| Reserves | 7789866424405752 | 7789809997399209 | -56,427 ADA (too high) |
| Treasury | 1537353372599546 | 1537376975934454 | +23,603 ADA (too low) |
| Deposits | 4376648000000 | 4373648000000 | -3,000 ADA (too high) |

Additional issues:
- Withdrawal underflow errors (~0.5% less rewards than expected)
- SPDD stake mismatch (~142k ADA difference with 1077 pools affected)

---

## Code Changes Summary (vs main branch)

### Files Modified (22 files, +1778/-858 lines)

| File | Changes | Purpose |
|------|---------|---------|
| `epoch_snapshot.rs` | +161 | SPO registration check, `from_raw_with_registration_check()` |
| `delegation_state.rs` | +395 (new) | Dedicated d_state parsing module |
| `governance.rs` | +122 | Enacted treasury withdrawals tracking |
| `mark_set_go.rs` | +168 | Corrected two_previous snapshot references |
| `reward_snapshot.rs` | +71 | Pulser member rewards extraction |
| `streaming_snapshot.rs` | +320/-320 | Multiple bootstrap parsing fixes |
| `monetary.rs` | +6 | floor() in η calculation |
| `state.rs` | +150 | Pool deposit tracking, DRep handling |
| `rewards.rs` | +17 | Reward calculation adjustments |

---

## Completed Fixes

### Fix 1: Pool Deposit Double-Counting
**Problem:** Pools retiring and re-registering had deposits counted twice.
**Root Cause:** At epoch boundary, `enter_epoch()` removes pools from `self.spos`. Later, `handle_spo_state()` sees them as "new".
**Solution:** Added `just_retired_pool_ids` field to track pools that retired this epoch.

### Fix 2: DRep Deposit Handling
**Problems:**
1. Bootstrap incorrectly subtracted DRep deposits from `us_deposited`
2. DRep re-registration caused double-counting

**Solution:**
1. Removed DRep deposit subtraction from bootstrap
2. Added registration tracking for truly NEW registrations only

### Fix 3-4: SPO Leader Rewards Registration Check
**Problem:** `two_previous_reward_account_is_registered` was hardcoded to `true` during bootstrap.
**Solution:** Added `from_raw_with_registration_check()` to properly check reward account registration against previous snapshot.

**Spec Reference (Figure 48):**
```
rewards = addrsrew ◁ potentialRewards   // Filter to ONLY registered reward accounts
```

### Fix 5: Enacted Treasury Withdrawals
**Problem:** Accounts with enacted treasury withdrawals incorrectly receiving pulsing rewards.
**Solution:** Skip pulsing rewards for accounts in `es_withdrawals`.

### Fix 6: Missing Member Rewards from Pulser
**Problem:** When reward calculation is in "Pulsing" state, member rewards from `pulser.reward_ans.accum_rewards` were skipped.
**Solution:** Parse and combine both leader rewards AND member rewards from pulser.

### Fix 7: Incorrect `two_previous` Snapshot Reference
**Problem:** Mark snapshot was using Set (epoch N-2) instead of Go (epoch N-3).
**Solution:** Corrected snapshot references:
- Mark (N-1) uses Go (N-3) as two_previous
- Set (N-2) and Go (N-3) use None

### Fix 13: floor() in η Calculation
**Problem:** Expected blocks calculation didn't use floor() as specified.
**Spec (Figure 51):**
```
η = blocksMade / floor((1 - d) * slotsPerEpoch * activeSlotCoeff)
```
**Solution:** Added `.with_scale(0)` to floor the expected_blocks calculation.

---

## Shelley Ledger Specification - Key Rules

### Snapshot Timing (Mark/Set/Go)
From Figure 38-39:
- **Mark** = current epoch snapshot (newest)
- **Set** = previous epoch snapshot (epoch - 1)
- **Go** = two epochs ago snapshot (epoch - 2) - **used for rewards calculation**

At epoch boundary:
```
pstakego := pstakeset
pstakeset := pstakemark
pstakemark := fresh_stake_calculation
feeSS := fees
```

### Reward Calculation (Figure 48: rewardOnePool)

**Maximum pool reward (Figure 46):**
```
maxPool pp R σ pr = ⌊(R / (1 + a0)) · (σ' + p' · a0 · ((σ' - p' · (z0 - σ') / z0) / z0))⌋

where:
  a0 = influence pp
  z0 = 1/nopt
  σ' = min(σ, z0)    // capped pool stake
  p' = min(pr, z0)   // capped pledge ratio
```

**Apparent performance (Figure 46):**
```
mkApparentPerformance d σ n N = {
  β/σ   if d < 0.8
  1     otherwise
}
where β = n / max(1, N)
```

**Leader reward (Figure 47):**
```
roperator f̂ pool s σ = {
  f̂                                    if f̂ ≤ c
  c + ⌊(f̂ - c) · (m + (1-m) · s/σ)⌋   otherwise
}
```

**Member reward (Figure 47):**
```
rmember f̂ pool t σ = {
  0                            if f̂ ≤ c
  ⌊(f̂ - c) · (1 - m) · t/σ⌋   otherwise
}
```

### Reward Update Application (Figure 52: applyRUpd)

**Critical: Deregistered accounts lose rewards to treasury:**
```
regRU = (dom rewards) ◁ rs        // Restricted to registered accounts
unregRU = (dom rewards) ⊲/ rs     // Accounts no longer registered
unregRU' = Σ c (for c in unregRU) // Sum of unregistered rewards

treasury' = treasury + Δt + unregRU'
reserves' = reserves + Δr
rewards' = rewards ∪+ regRU
```

### Pool Retirement Deposits (Figure 41: POOLREAP)

**Deposit refund rules:**
```
refunds = dom rewards ◁ rewardAcnts'     // Refunds to REGISTERED accounts
mRefunds = dom rewards ⊲/ rewardAcnts'   // Refunds to UNREGISTERED accounts
refunded = Σ(c ∈ refunds)
unclaimed = Σ(c ∈ mRefunds)

deposits' = deposited - (unclaimed + refunded)
treasury' = treasury + unclaimed         // Unclaimed goes to treasury
rewards' = rewards ∪+ refunds
```

### Epoch Boundary Order (Figure 45)
1. **SNAP** - Creates stake snapshots, sets `deposited = obligation`
2. **POOLREAP** - Reaps retired pools, refunds deposits
3. **NEWPP** - Applies new protocol parameters

### Active Delegations Filter (Figure 37)
```
activeDelegs = (dom rewards) ◁ delegations ▷ (dom poolParams)
```
Only registered stake accounts delegating to registered pools receive rewards.

---

## Known Errata from Spec (Section 17)

### 17.1 Total Stake Calculation
**Issue:** `createRUpd` uses current reserves instead of previous epoch's value.

### 17.2 Active Stake Registrations
**Issue:** Filtering by current active reward accounts prevents recently deregistered accounts from re-registering to claim rewards.

### 17.4 Reward Aggregation (CRITICAL)
**Issue:** Accounts receiving both member AND leader rewards only get one.
**Solution:** Use aggregating union `∪+` in `rewardOnePool` and `reward` functions.
**Fix:** Corrected at Allegra hard fork; 64 stake addresses were reimbursed via MIR certificates.

### 17.7 Deposit Tracking
**Issue:** Original spec doesn't track individual deposits; returns based on current parameters.
**Solution:** Individual deposits now tracked in DState; exact amount always returned.

---

## Remaining Investigation Areas

### 1. Reserves Too High (-56,427 ADA difference)
**Possible causes:**
- Reward pot calculation using wrong reserves value (Errata 17.1)
- η calculation issues
- Fee snapshot timing

### 2. Treasury Too Low (+23,603 ADA difference)
**Possible causes:**
- Deregistered account rewards not going to treasury correctly
- Pool retirement unclaimed deposits handling
- Treasury withdrawal double-application

### 3. Deposits Too High (-3,000 ADA difference)
**Possible causes:**
- Pool deposits for re-registering pools
- DRep deposit edge cases
- Stake key deposit tracking

### 4. SPDD Stake Mismatch (~142k ADA, 1077 pools)
**Possible causes:**
- Stake distribution using wrong snapshot
- Delegation filtering issues
- UTxO stake aggregation errors

---

## Spec References Quick Guide

| Topic | Figure | Page |
|-------|--------|------|
| Delegation rules | 23 | 35-37 |
| Pool registration | 25 | 39-40 |
| Stake distribution | 37 | 49-50 |
| Snapshot structure | 38-39 | 51-52 |
| POOLREAP | 41 | 53-54 |
| maxPool | 46 | ~61 |
| mkApparentPerformance | 46 | ~61 |
| roperator/rmember | 47 | ~61 |
| rewardOnePool | 48 | ~62 |
| RewardUpdate | 50 | ~65 |
| createRUpd | 51 | 66 |
| applyRUpd | 52 | 67 |
| EPOCH transition | 45 | ~60 |
| NEWEPOCH | 57 | 72 |
| RUPD timing | 62 | 75 |
| Preservation of value | - | 106-109 |
| Errata | 17 | 114-115 |
