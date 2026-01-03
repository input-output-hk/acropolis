# Rewards Payout Investigation - Hypotheses and Fixes

## Summary of Issues Observed

From log analysis, three main categories of errors occur starting at epoch 509:

| Category | Observed | Expected | Difference |
|----------|----------|----------|------------|
| Reserves | 7789866424405752 | 7789809997399209 | -56,427 ADA (too high) |
| Treasury | 1537353372599546 | 1537376975934454 | +23,603 ADA (too low) |
| Deposits | 4376648000000 | 4373648000000 | -3,000 ADA (too high) |

Additionally:
- Withdrawal underflow errors (~0.5% less rewards than expected)
- SPDD stake mismatch (~142k ADA difference with 1077 pools affected)

---

## Fixes Applied (Completed)

### Fix 1: Pool Deposit Double-Counting
**Status:** COMPLETED

**Problem:** Pools retiring and re-registering had deposits counted twice.

**Root Cause:** At epoch boundary, `enter_epoch()` removes pools from `self.spos`. Later, `handle_spo_state()` sees them as "new" since they're not in the map anymore.

**Solution:** Added `just_retired_pool_ids` field to track pools that retired this epoch, excluding them from deposit counting.

**Files:** `modules/accounts_state/src/state.rs`

---

### Fix 2: DRep Deposit Handling
**Status:** COMPLETED

**Problems:**
1. Bootstrap incorrectly subtracted DRep deposits from `us_deposited`
2. DRep re-registration caused double-counting of deposits

**Solution:**
1. Removed DRep deposit subtraction from bootstrap
2. Added registration tracking for DReps to only add deposit for truly NEW registrations

**Files:** `common/src/snapshot/streaming_snapshot.rs`, `modules/accounts_state/src/state.rs`

---

### Fix 3-4: SPO Leader Rewards Registration Check
**Status:** COMPLETED

**Problem:** `two_previous_reward_account_is_registered` was hardcoded to `true` during bootstrap.

**Solution:** Added `from_raw_with_registration_check()` to properly check reward account registration against previous snapshot.

**Files:** `common/src/epoch_snapshot.rs`, `common/src/snapshot/mark_set_go.rs`, `common/src/snapshot/streaming_snapshot.rs`

---

### Fix 5: Enacted Treasury Withdrawals
**Status:** COMPLETED

**Problem:** Accounts with enacted treasury withdrawals were incorrectly receiving pulsing rewards (double-counting).

**Solution:** Skip pulsing rewards for accounts that have enacted withdrawals in `es_withdrawals`.

**Files:** `common/src/snapshot/governance.rs`, `common/src/snapshot/streaming_snapshot.rs`

---

### Fix 6: Missing Member Rewards from Pulser
**Status:** COMPLETED

**Problem:** When reward calculation is in "Pulsing" state (not Complete), member rewards from `pulser.reward_ans.accum_rewards` were being skipped entirely.

**Solution:** Parse and combine both leader rewards from `snapshot.leaders` AND member rewards from `pulser.reward_ans.accum_rewards`.

**Files:** `common/src/snapshot/reward_snapshot.rs`, `common/src/snapshot/streaming_snapshot.rs`

---

### Fix 7: Incorrect `two_previous` Snapshot Reference
**Status:** COMPLETED

**Problem:** Mark snapshot was using Set (epoch N-2) as "two previous" instead of Go (epoch N-3).

**Solution:** Corrected snapshot references:
- Mark (N-1) uses Go (N-3) as two_previous
- Set (N-2) and Go (N-3) use None (not available)

**Files:** `common/src/snapshot/mark_set_go.rs`

---

## Additional Fixes (Recently Completed)

### Fix 9: Unregistered Accounts Missing Pulsing Rewards
**Status:** COMPLETED

**Problem:** Unregistered stake accounts don't receive pulsing rewards during bootstrap.

**Location:** `common/src/snapshot/streaming_snapshot.rs:1277-1303`

**Solution:** Already implemented - the code checks `pulsing_result.rewards.get(credential)` for unregistered accounts and applies pulsing rewards correctly.

---

### Fix 10: Deregistered Delegator Rewards to Treasury
**Status:** COMPLETED

**Problem:** Delegators who deregister between snapshot and reward application need their rewards sent to treasury.

**Location:** `modules/accounts_state/src/state.rs:752-757`

**Solution:** Already implemented in `complete_previous_epoch_rewards_calculation` - when reward account is deregistered, the reward is sent to treasury per spec (Figure 52 - applyRUpd).

---

### Fix 11: Performance Calculation Denominator (FALSE POSITIVE)
**Status:** NO FIX NEEDED - Code is Correct

**Analysis:** Initial review incorrectly identified this as a bug. Re-reading the Shelley spec (Figure 48):
- `rewardOnePool` takes BOTH `σ` (relative to total supply) AND `σa` (relative to active stake)
- `mkApparentPerformance` uses `σa`, not `σ`
- The implementation correctly uses `total_active_stake` for the performance calculation

The spec says: `appPerf = mkApparentPerformance (d pp) σa n N` where `σa = pool_stake / total_active_stake`.

---

### Fix 12: Fee Snapshot (feeSS)
**Status:** NEEDS INVESTIGATION - LOW PRIORITY

**Observation:** The fee snapshot is parsed from CBOR but discarded at `streaming_snapshot.rs:2073`.

**Analysis:** In live operation, `total_fees` comes from `EpochActivityMessage` which correctly represents fees from the previous epoch. During bootstrap, the situation is more complex but may not cause issues if the first `EpochActivityMessage` after bootstrap has correct values.

**Location:** `common/src/snapshot/streaming_snapshot.rs:2073`

---

### Fix 13: floor() in η Calculation
**Status:** COMPLETED

**Problem:** Expected blocks calculation didn't use floor() as specified.

**Location:** `modules/accounts_state/src/monetary.rs:78-84`

**Spec:**
```
η = blocksMade / floor((1 - d) * slotsPerEpoch * activeSlotCoeff)
```

**Solution:** Added `.with_scale(0)` to floor the expected_blocks calculation before division.

---

## Notes on Correct Behavior

### SPO Reward Accounts Not in DState

Investigation confirmed that SPO reward accounts not registered in DState **correctly do not receive leader rewards** per Shelley spec (Figure 48):
```
rewards = addrsrew ⨃ potentialRewards   // Filter to ONLY registered reward accounts
```

This is by design - if an SPO deregisters their reward stake key, they forfeit leader rewards. The "not paid" log messages for such SPOs are correct behavior.

---

## Issue Tracker Summary

| Fix # | Description | Status | Priority |
|-------|-------------|--------|----------|
| 1 | Pool deposit double-counting | COMPLETED | - |
| 2 | DRep deposit handling | COMPLETED | - |
| 3-4 | SPO leader rewards registration | COMPLETED | - |
| 5 | Enacted treasury withdrawals | COMPLETED | - |
| 6 | Missing Pulser member rewards | COMPLETED | - |
| 7 | Wrong two_previous snapshot | COMPLETED | - |
| 9 | Unregistered accounts pulsing rewards | COMPLETED | - |
| 10 | Deregistered delegator rewards to treasury | COMPLETED | - |
| 11 | Performance wrong denominator | NO FIX NEEDED | - |
| 12 | Fee snapshot not used | NEEDS INVESTIGATION | LOW |
| 13 | Missing floor() in η | COMPLETED | - |
| 14 | Aggregating union for dual rewards (Errata 17.4) | COMPLETED | - |
| 15 | Pool reward account incorrectly excluded | COMPLETED | - |
| 16 | Bootstrap vs live snapshot comparison | DIAGNOSTIC ADDED | - |
| 17 | Pulsing rewards completeness validation | DIAGNOSTIC ADDED | - |
| 18 | Previous protocol params not set at bootstrap | COMPLETED | - |
| 19 | Skip rewards task creation at first bootstrap epoch | COMPLETED | - |
| 22 | Leader rewards registration check timing | COMPLETED | - |

---

## New Fixes (January 2026)

### Fix 14: Aggregating Union for Dual Rewards (Errata 17.4)
**Status:** COMPLETED

**Problem:** SPOs whose reward account also delegates to their own pool were only receiving one type of reward (either leader OR member), not both aggregated together.

**Spec Reference:** Errata 17.4 - Fixed at Allegra hard fork with 64 stake addresses reimbursed via MIR certificates.

**Solution:** Implemented aggregating union (∪+) in `calculate_spo_rewards()` using a HashMap to accumulate rewards by account before creating final RewardDetails.

**Files:** `modules/accounts_state/src/rewards.rs`

---

### Fix 15: Pool Reward Account Incorrectly Excluded from Member Rewards
**Status:** COMPLETED

**Problem:** The pool reward account was being excluded from member rewards, but the spec (Figure 48) only excludes pool OWNERS (`hk ∉ poolOwners pool`), not the pool reward account.

**Solution:** Removed the incorrect exclusion. Now the pool reward account can receive member rewards if it delegates to the pool, and these will be aggregated with leader rewards per Fix 14.

**Files:** `modules/accounts_state/src/rewards.rs`

---

### Fix 18: Previous Protocol Parameters Not Set at Bootstrap
**Status:** COMPLETED

**Problem:** At bootstrap, `previous_protocol_parameters` was never set because:
1. The publisher was discarding `previous_reward_params` from the snapshot (using `_` for the parameters)
2. When `handle_parameters()` was first called, it set `previous_protocol_parameters = protocol_parameters.clone()`, but both were `None`

This meant reward calculations at the first epoch boundary after bootstrap might use fallback params incorrectly.

**Solution:**
1. Updated `ProtocolParametersBootstrapMessage` to include `previous_reward_params` and `current_reward_params`
2. Updated the snapshot publisher to send these params instead of discarding them
3. Updated `handle_parameters()` in accounts_state to set `previous_protocol_parameters` to the new params if both previous and current are None (bootstrap case)

**Files:**
- `common/src/messages.rs` - Added `previous_reward_params` and `current_reward_params` to message
- `common/src/types.rs` - Added serde derives to `RewardParams`
- `modules/snapshot_bootstrapper/src/publisher.rs` - Now sends reward params instead of discarding
- `modules/accounts_state/src/state.rs` - Bootstrap-aware handling in `handle_parameters()`

---

## Diagnostic Logging Added

### Bootstrap vs Live Snapshot Comparison (Fix 16)
Added detailed comparison logging at the first epoch boundary after bootstrap to identify state drift.

**Location:** `modules/accounts_state/src/state.rs`

### Pulsing Rewards Completeness Validation (Fix 17)
Added validation to check if extracted rewards match expected rewards (r - delta_t1).

**Location:** `common/src/snapshot/streaming_snapshot.rs`

---

## Logs to Watch For

When running, look for these log messages to diagnose issues:

### Bootstrap Snapshot Summary
```
[BOOTSTRAP SNAPSHOT] Mark (epoch X): Y SPOs, Z total blocks, reserves=..., treasury=..., deposits=...
[BOOTSTRAP SNAPSHOT] Set (epoch X): Y SPOs, Z total blocks
[BOOTSTRAP SNAPSHOT] Go (epoch X): Y SPOs, Z total blocks
```

### Bootstrap to Live Transition
```
[BOOTSTRAP->LIVE TRANSITION] First epoch boundary after bootstrap (bootstrap epoch X)
[BOOTSTRAP->LIVE TRANSITION] Creating live snapshot for epoch X to replace bootstrap Mark (epoch Y)
```

### Snapshot Comparison (if differences found)
```
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: Total stake differs by X lovelace (Y ADA)
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: Pool count differs - old=X, new=Y
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: X pools with stake differences, total diff=Y
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: Pool <id> stake diff X (Y ADA): old=..., new=...
```

### Pulsing Rewards Validation
```
[PULSING VALIDATION] Rewards complete: extracted=X matches expected=Y
[PULSING VALIDATION] Rewards difference within tolerance: extracted=X, expected=Y, diff=Z (P%)
[PULSING VALIDATION WARN] Rewards may be INCOMPLETE: extracted=X, expected=Y, diff=Z (P%)
```

### Aggregating Union (Errata 17.4)
```
SPO <id> reward account <addr> receives both leader (X) AND member (Y) rewards - aggregating per Errata 17.4
```

### Reward Calculation
```
Pool <id> reward calculation: pool_stake=..., relative_pool_stake=..., pool_performance=...
Reward split (roperator + rmember): fixed_cost=..., margin_cost=..., leader_reward=..., to_delegators=...
```

---

## Issue Under Investigation: Incomplete Epoch Blocks at Bootstrap

### Fix 19: Skip Rewards Task Creation at First Epoch After Bootstrap
**Status:** REVERTED (caused SPDD mismatch)

**Original Problem:** At the 507→508 bootstrap transition, something is being skipped or miscalculated that causes epoch 508 SPDD to be wrong by ~117k ADA.

**Why Fix 19 Was Wrong:**

The fix incorrectly assumed that skipping rewards task creation at 507→508 would prevent double-counting. However:

- **Bootstrap pulsing rewards** are for epoch 506 (calculated during epoch 507)
- **Rewards task at 507→508** calculates epoch 507 rewards (applied at 508→509)

These are **DIFFERENT EPOCHS**! There is no double-counting risk.

By skipping task creation at 507→508:
1. No rewards task exists for epoch 507
2. At 508→509, `complete_previous_epoch_rewards_calculation()` finds no task
3. Epoch 507 rewards are **never applied**
4. SPDD for epoch 508 is missing ~8.28 million ADA in rewards

**Solution Applied:**

Reverted the skip_task_creation logic. We now always create the rewards task at each epoch boundary, even the first one after bootstrap.

**Correct Flow at First Epoch After Bootstrap (507→508):**
1. `skip_first_epoch_rewards = true` for **rewards application** (pulsing rewards already applied during bootstrap)
2. Rewards task for epoch 507 IS created (using Mark=507 blocks, Go=505 stake)
3. Task will be applied at 508→509 boundary

**At 508→509:**
1. `skip_first_epoch_rewards = false`
2. `complete_previous_epoch_rewards_calculation(skip=false)` applies epoch 507 rewards from the task
3. SPDD for epoch 508 now correctly includes epoch 507 rewards

**Files Modified:**
- `modules/accounts_state/src/state.rs` - Removed `skip_task_creation` parameter from `enter_epoch()` and `handle_epoch_activity()`
- `modules/accounts_state/src/accounts_state.rs` - Removed `skip_first_epoch_rewards` from `handle_epoch_activity()` call

---

### Fix 20: Unpaid Leader Rewards Handling (CORRECTED)
**Status:** CORRECTED - Unpaid stays in RESERVES, not treasury

**Original Problem:** When an SPO's reward account is not registered, the calculated leader reward was being logged as "not paid" but not tracked anywhere.

**INCORRECT Initial Fix:** Added unpaid leader rewards to treasury and subtracted from reserves.

**Why It Was Wrong:** Per Shelley spec (Figures 48, 51, 52):
- `rewards = addrsrew ✁ potentialRewards` - Rewards for unregistered accounts are filtered out at CALCULATION time
- `Δr2 = R - sum(rs)` - The leftover (unallocated rewards) goes back to RESERVES
- `unregRU` (which goes to treasury) is for rewards that WERE calculated but then the account got deregistered BEFORE application

So:
- **Unregistered at calculation time** → Leader reward stays in reserves (never calculated)
- **Registered at calculation, deregistered at application** → Goes to treasury (this is `unregRU`)

**Correct Solution:**
1. Renamed `total_unpaid_to_treasury` → `total_unpaid_leader_rewards` for accuracy (these stay in reserves, not treasury!)
2. Do NOT move unpaid from reserves to treasury
3. The existing code at line 1065 (`self.pots.treasury += reward.amount` for deregistered accounts at application time) correctly handles `unregRU`

**Files:**
- `modules/accounts_state/src/rewards.rs` - Renamed field and updated comments to clarify the distinction
- `modules/accounts_state/src/state.rs` - Removed incorrect reserves/treasury adjustment, updated logging

**Naming Clarification (January 2026):**
- `total_unpaid_leader_rewards` = leader rewards NOT calculated because SPO reward account wasn't registered at calc time → stays in RESERVES
- `unregRU` = rewards that WERE calculated but account deregistered before application → goes to TREASURY
- These are fundamentally different! The old naming was confusing and led to bugs.

---

### Fix 21: DRep Deregistration Refunds to Stake Address
**Status:** COMPLETED

**Problem:** When a DRep deregisters, their deposit refund was not being credited to their stake address, causing SPDD mismatch.

**Solution:** Added code to credit the refund to the DRep's stake address (similar to pool deposit refunds).

**Files:** `modules/accounts_state/src/state.rs`

---

### Fix 22: Leader Rewards Registration Check Timing
**Status:** COMPLETED

**Problem:** Leader rewards for SPOs were being skipped based on `two_previous_reward_account_is_registered` which checks registration status from 2-3 epochs ago. This caused ~101k ADA in leader rewards to incorrectly stay in reserves.

**Root Cause:** The code was using historical snapshot data (`performance_spo.two_previous_reward_account_is_registered`) instead of checking current registration status.

**Spec Reference:** Per Shelley spec (Figure 48):
```
isRRegistered = rewardAcnt ∈ dom (rewards pp dstate)
```
This checks if the reward account is in `dstate` - the **current** delegation state at calculation time, NOT historical snapshot data.

**Solution:**
1. Added `registered_stake_addresses: &HashSet<StakeAddress>` parameter to `calculate_rewards()`
2. Extract currently registered addresses from `stake_addresses` before spawning the rewards task
3. Check `registered_stake_addresses.contains(&staking_spo.reward_account)` instead of using historical flag

**Files:**
- `modules/accounts_state/src/rewards.rs` - Updated registration check logic
- `modules/accounts_state/src/state.rs` - Extract and pass current registered addresses

**Impact:** This should significantly reduce the reserves discrepancy by correctly paying leader rewards to SPOs whose reward accounts ARE registered now (even if they weren't registered 2 epochs ago).

---

## Reference Data: Bootstrap Epoch Transitions

### Epoch 506 (in snapshot)
- Total Fees: **59,492 ADA** (59,492,203,551 lovelace)
- These fees are part of the pulsing_rew_update in the bootstrap snapshot
- Already accounted for in pot deltas

### Epoch 507 (first live epoch after bootstrap)
- Blocks: 20,803
- Total Fees: **116,094 ADA** (116,094,310,914 lovelace)
- Rewards Distributed: **8,556,666 ADA**
- At 507→508 transition:
  - Fees from epoch 507 must be added to reward pot via `EpochActivityMessage`
  - Monetary expansion + fees should yield ~8.5M ADA in rewards
  - This is the first epoch boundary where live reward calculation kicks in

### Verification Points
At epoch 509 verification:
1. Rewards from epoch 507 (8.5M ADA) should have been applied at 508→509 boundary
2. Fees from epoch 507 (116k ADA) should be reflected in the reward pot calculation
3. Leader rewards should now be paid to SPOs with currently registered accounts

---

## Known Issues (Pending)

### Issue: Enacted Governance Actions Not Handled by accounts_state
**Status:** CRITICAL - NEEDS FIX

**Problem:** When governance actions are enacted (e.g., TreasuryWithdrawals), the `accounts_state` module doesn't receive or process them.

**Current Flow:**
1. `governance_state` produces `GovernanceOutcomesMessage` with enacted actions
2. `parameters_state` consumes it (for protocol param updates only)
3. **`accounts_state` does NOT consume this message!**

**Result:**
- Treasury withdrawals are enacted but treasury is never decreased
- Explains why treasury is ~11k ADA TOO LOW (withdrawals not subtracted)
- Also explains why reserves is ~78k ADA TOO HIGH (the withdrawn amounts stay in reserves)

**Required Fix:**
1. `accounts_state` must subscribe to `GovernanceOutcomesMessage`
2. For `TreasuryWithdrawal` outcomes:
   - Subtract total withdrawal amount from `pots.treasury`
   - Credit each recipient's stake address with their withdrawal amount

**Files to Modify:**
- `modules/accounts_state/src/accounts_state.rs` - Subscribe to GovernanceOutcomes
- `modules/accounts_state/src/state.rs` - Add `handle_governance_outcomes()` method

---

### Issue: Governance Proposal Deposits Not Tracked During Live Processing
**Status:** NEEDS FIX

**Problem:** At bootstrap, governance proposal deposits are explicitly subtracted from `us_deposited`. But during live processing:
- No code adds governance proposal deposits when proposals are submitted
- No code refunds governance proposal deposits when proposals are enacted/expired

This asymmetry causes the deposits pot to drift over time.

**Files to Modify:**
- `common/src/messages.rs` - Add governance deposit message
- `modules/governance_state/src/state.rs` - Send deposit messages
- `modules/accounts_state/src/state.rs` - Handle deposit messages
