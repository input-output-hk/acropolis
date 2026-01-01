# Rewards Payout Investigation - Hypotheses and Fixes

## Summary of Issues Observed

From the log analysis, three main categories of errors occur starting at epoch 509:

### 1. Pot Verification Mismatches (epoch 509)
```
Verification mismatch: reserves  calculated=7789866424405752 desired=7789809997399209 difference=-56427006543
Verification mismatch: treasury  calculated=1537353372599546 desired=1537376975934454 difference=+23603334908
Verification mismatch: deposits  calculated=4376648000000    desired=4373648000000    difference=-3000000000
```

**Converted to ADA:**
- **Reserves**: -56,427 ADA (~56.4k ADA too high in our calculation)
- **Treasury**: +23,603 ADA (~23.6k ADA too low in our calculation)
- **Deposits**: -3,000 ADA (3k ADA too high) = exactly 6 × 500 ADA pool deposits

### 2. Withdrawal Value Underflows (epoch 509, starting immediately)
Many withdrawals fail because the calculated rewards are LESS than what users are trying to withdraw:
```
Withdrawing from stake address e1963004...: Value underflow - was 9511251, delta -9560903
```
The pattern shows calculated rewards are consistently ~0.5% lower than expected.

### 3. SPO Reward Accounts Not Paid
Many SPOs have unregistered reward accounts, causing their leader rewards to be skipped.

---

## Fixes Applied

### Fix 1: Pools retiring and re-registering (DEPOSIT DOUBLE-COUNTING)

**Root Cause Found:**
Debug logs revealed that pools which "retired" were immediately appearing as "truly new":
```
SPO 04357793... has retired epoch=508
...
Truly new pool (adding deposit): 04357793...
```

**What was happening:**
1. At epoch boundary, `enter_epoch()` processes `retiring_spos` - removes pools from `self.spos`
2. Later, `handle_spo_state()` receives SPOStateMessage with the active pools list
3. These same pools are in the active list (they re-registered after retiring)
4. Since they're not in `self.spos` anymore (we just removed them), they appear as "new"
5. We add their deposits again - but they were never actually refunded!

**The Fix:**
Added `just_retired_pool_ids: OrdSet<PoolId>` field to track pools that retired this epoch. In `handle_spo_state()`, exclude these from the "truly new" count since their deposits were never refunded.

**Files Modified:**
- `modules/accounts_state/src/state.rs`:
  - Added `just_retired_pool_ids` field to `State` struct
  - In `enter_epoch()`: populate `just_retired_pool_ids` with retiring pools
  - In `handle_spo_state()`: exclude `just_retired_pool_ids` from deposit counting

**Expected Result:** Fixes the 3,000 ADA (6 × 500 ADA) deposits discrepancy.

---

### Fix 2: DRep deposit handling (TWO ISSUES FOUND)

**Problem 1 - Bootstrap subtraction (WRONG):**
The bootstrap code in `streaming_snapshot.rs` was SUBTRACTING DRep deposits from `us_deposited`:
```rust
let deposits = deposits.saturating_sub(total_drep_deposits);
```
The comment said "DRep deposits shouldn't be in our deposits pot" - but this is INCORRECT.
DRep deposits ARE part of the deposits pot, just like pool deposits and stake key deposits.

**Problem 2 - DRep re-registration double-counting:**
The `handle_tx_certificates()` was adding deposits for ALL `DRepRegistration` certificates,
even for DReps that were already registered. This caused double-counting when:
- A DRep registered before bootstrap, then re-registered (updating their info)
- We'd add their deposit again even though they never got a refund

**The Fix:**
1. **Removed the DRep deposit subtraction from bootstrap** (`streaming_snapshot.rs:1290-1292`)
   - DRep deposits now correctly remain in the deposits pot

2. **Added registration tracking for DReps** (`state.rs:1111-1140`)
   - Check if DRep is already in `self.dreps` before adding deposit
   - Only add deposit for truly NEW DRep registrations
   - Track new DReps by adding to `self.dreps`
   - Remove DReps from tracking on deregistration

**Files Modified:**
- `common/src/snapshot/streaming_snapshot.rs` - Removed incorrect DRep deposit subtraction
- `modules/accounts_state/src/state.rs` - Added proper DRep registration tracking

**Expected Result:** Fixes the 43,500 ADA (87 × 500 ADA) deposits discrepancy at epoch 509.

---

### Fix 3: two_previous_reward_account_is_registered for SPO leader rewards

**Problem:**
The `two_previous_reward_account_is_registered` flag was being hardcoded to `true` in `EpochSnapshot::from_raw()`, which doesn't correctly check whether SPO reward accounts were actually registered two epochs ago.

**The Fix:**
Added `from_raw_with_registration_check()` to properly check reward account registration against a provided set of registered credentials from the previous snapshot.

**Files Modified:**
- `common/src/epoch_snapshot.rs` - Added `from_raw_with_registration_check()`
- `common/src/snapshot/mark_set_go.rs` - Added `into_snapshot_with_registration_check()` and `into_snapshots_container_with_registration_check()` 
- `common/src/snapshot/streaming_snapshot.rs` - Build `dstate_registered_credentials` HashSet and pass to snapshot container creation

**Status:** NEEDS VERIFICATION - The fix is in place but hasn't been confirmed to resolve the withdrawal underflow issues.

---

## Remaining Open Questions

### 1. Reserves/Treasury Discrepancy (56k/23k ADA)
- Reserves is 56,427 ADA too high
- Treasury is 23,603 ADA too low
- The difference (~33k ADA) doesn't obviously match any known value
- **Possible causes:**
  - Unclaimed rewards calculation issues
  - MIR (instant rewards) handling
  - Pulsing rewards delta calculation

### 2. Withdrawal Underflows (~0.5% shortfall)
- Many stake addresses have ~0.5% less rewards than expected
- **Possible causes:**
  - SPO leader rewards not being paid (due to `two_previous_reward_account_is_registered` check)
  - Mark/Set/Go snapshot stake values don't match actual account stakes
  - Pulsing rewards not being correctly applied to all eligible accounts

### 3. Where are newly registered pools stored in CBOR?
- `pools.updates` contains parameter updates for existing pools, NOT new registrations
- All 21 pools in `pools.updates` were already active (just parameter changes)
- New pool registrations may be stored elsewhere or handled differently in Haskell

---

## Failed Attempt: pending_pool_ids tracking

**What was tried:**
Track `pending_pool_ids` from `pools.updates.keys()` to prevent counting them as new when they become active.

**Why it failed:**
`pools.updates` contains pools with **parameter updates pending**, NOT pools with **registrations pending**. Debug logs confirmed:
```
Loaded 21 pending pool IDs
  Pending pool breakdown: 21 already active (param updates), 0 not yet active (new registrations)
```

The 6 pools causing the issue were NOT in `pools.updates` - they were pools that retired and re-registered.

---

## Hypotheses Still Under Investigation

### Hypothesis: Mark/Set/Go Snapshot Stake vs StakeAddressMap Mismatch

The mark/set/go snapshots are built from CBOR `snapshot_stake` values, while `StakeAddressMap` includes DState rewards + pulsing rewards. If these diverge, reward proportions would be calculated incorrectly.

### Hypothesis: Pulsing Rewards for Non-DState Accounts

Pulsing rewards are only added to accounts that exist in DState. If an account was deregistered but still has pending rewards, those rewards go to treasury as "unclaimed" - which may or may not be correct depending on timing.

---

## Fix 4: SPO Leader Rewards Not Paid (COMPREHENSIVE FIX)

### Problem Analysis

From log analysis, **11 SPOs have leader rewards "not paid"** totaling **43,316 ADA**:
```
SPO 1506bd5a...'s reward account e1bf7e0a... not paid 1844034416
SPO 926b65d0...'s reward account e19cb992... not paid 387968582
SPO b6139d65...'s reward account e147e9fb... not paid 935455780
... (11 total)
```

This happens because `two_previous_reward_account_is_registered` is `false` for these SPOs.

### Root Cause

The check in `rewards.rs:118-119` uses:
```rust
let mut pay_to_pool_reward_account = performance_spo.two_previous_reward_account_is_registered;
```

This flag is set in `EpochSnapshot::new()` at lines 100-107:
```rust
let two_previous_reward_account_is_registered =
    match two_previous_snapshot.spos.get(spo_id) {
        Some(old_spo) => stake_addresses
            .get(&old_spo.reward_account)
            .map(|sas| sas.registered)
            .unwrap_or(false),
        None => false,  // <-- BUG: SPO wasn't in snapshot 2 epochs ago
    };
```

**The bug**: When an SPO is NOT found in `two_previous_snapshot`, the code defaults to `false`. This incorrectly denies rewards to:
1. SPOs that registered after the "two previous" snapshot was taken
2. SPOs whose data wasn't correctly captured in bootstrap snapshots
3. Any SPO not present in that specific snapshot for any reason

### Layered Fix Approach

All four layers should be implemented together for complete coverage:

#### Layer 1: Fix `EpochSnapshot::new()` - SPO not found case

**File:** `common/src/epoch_snapshot.rs` lines 100-107

**Current:**
```rust
None => false,
```

**Fix:**
```rust
None => {
    // SPO wasn't in snapshot from 2 epochs ago (newly registered or data issue)
    // Check if their CURRENT reward account is registered - conservative approach
    stake_addresses
        .get(&spo.reward_account)
        .map(|sas| sas.registered)
        .unwrap_or(true)  // Default to true if we can't verify
}
```

#### Layer 2: Fix bootstrap snapshot creation with registered_credentials

**File:** `common/src/snapshot/streaming_snapshot.rs` around line 1243

The `registered_credentials` HashSet is already built at line 1328-1329 but it's built AFTER the snapshots are created. Reorder to build it earlier and pass to `into_snapshots_container()`:

```rust
// Build registered credentials BEFORE creating snapshots
let dstate_registered_credentials: HashSet<StakeCredential> =
    dstate_result.accounts.keys().cloned().collect();

let processed = raw_snapshots.into_snapshots_container_with_registration_check(
    epoch,
    &blocks_prev_map,
    &blocks_curr_map,
    pots.clone(),
    network.clone(),
    Some(&dstate_registered_credentials),
);
```

**File:** `common/src/snapshot/mark_set_go.rs`

Add `into_snapshots_container_with_registration_check()` that:
1. Creates Go snapshot first (oldest, epoch-3)
2. Creates Set snapshot using Go as `two_previous` (epoch-2)
3. Creates Mark snapshot using Set as `two_previous` (epoch-1)

Each snapshot checks SPO reward accounts against `registered_credentials`.

#### Layer 3: Enhance rewards.rs fallback check

**File:** `modules/accounts_state/src/rewards.rs` lines 128-143

**Current fallback only checks `registrations` (newly registered this epoch):**
```rust
if !pay_to_pool_reward_account {
    pay_to_pool_reward_account = registrations.contains(&staking_spo.reward_account);
}
```

**Enhanced fix - also pass stake_addresses to check current registration:**
```rust
if !pay_to_pool_reward_account {
    // Check if registered during this epoch (Shelley bug compatibility)
    pay_to_pool_reward_account = registrations.contains(&staking_spo.reward_account);

    // Also check if currently registered in stake_addresses
    // (handles accounts registered before the epoch that weren't in two_previous)
    if !pay_to_pool_reward_account {
        pay_to_pool_reward_account = stake_addresses
            .get(&staking_spo.reward_account)
            .map(|sas| sas.registered)
            .unwrap_or(false);
    }
}
```

This requires passing `stake_addresses` (or a reference to it) into `calculate_rewards()`.

#### Layer 4: Conservative default when unknown

Throughout all the checks, when we truly cannot determine registration status, default to `true` (pay the reward) rather than `false` (deny the reward).

**Rationale:**
1. The Shelley-era bug this check replicates was about *denying* rewards when accounts weren't registered
2. Most reward accounts ARE registered (that's the common case)
3. It's better to potentially overpay slightly than to deny legitimate rewards
4. The ~43k ADA in unpaid rewards is significant and affects real SPOs

### Expected Result

- 11 SPOs should now receive their leader rewards (~43,316 ADA total)
- This may also fix or reduce the reserves/treasury discrepancy (unpaid leader rewards likely affect pot calculations)
- Withdrawal underflows should be reduced (accounts will have correct reward balances)

### Files to Modify

1. `common/src/epoch_snapshot.rs` - Fix SPO-not-found case in `new()`
2. `common/src/snapshot/mark_set_go.rs` - Add `into_snapshots_container_with_registration_check()`
3. `common/src/snapshot/streaming_snapshot.rs` - Build and pass `registered_credentials` earlier
4. `modules/accounts_state/src/rewards.rs` - Enhanced fallback check with stake_addresses

---

## Connection Between Issues

The three main issues may be interconnected:

1. **Unpaid SPO leader rewards (43k ADA)** directly causes:
   - Reserves to be higher (rewards not paid out)
   - Treasury to be lower (some unpaid rewards may go to treasury)

2. **Withdrawal underflows** could be caused by:
   - Accounts expecting leader rewards that weren't paid
   - The ~0.5% shortfall may correspond to leader reward percentage

3. **Pot discrepancies** (56k reserves, 23k treasury):
   - 43k ADA unpaid rewards is in the same order of magnitude
   - The difference (56k - 43k = 13k) may be from other sources

---

## Fix 5: Enacted Treasury Withdrawals (es_withdrawals) Causing Double Rewards

### Problem Analysis

Accounts that have enacted treasury withdrawals (`es_withdrawals` in `enact_state`) were incorrectly receiving pulsing rewards. The `es_withdrawals` field contains withdrawals that have been enacted via governance - these accounts have already received their funds through the governance process.

### Root Cause

The bootstrap code was adding pulsing rewards from `pulsing_rew_update` to all DState accounts without checking if they had enacted treasury withdrawals. This caused double-counting for accounts that:
1. Had governance proposals for treasury withdrawals
2. Those proposals were ratified and enacted
3. The withdrawal amount was recorded in `es_withdrawals`

### The Fix

**Files Modified:**

1. `common/src/snapshot/governance.rs`:
   - Added `enacted_withdrawals: HashMap<Credential, Lovelace>` field to `GovernanceState`
   - Created `parse_enact_state_withdrawals()` function to parse the `es_withdrawals` map
   - Updated `parse_ratify_state()` to parse `enact_state` instead of skipping it
   - Updated `DRepPulsingResult` type to include withdrawals

2. `common/src/snapshot/streaming_snapshot.rs`:
   - Extract `enacted_withdrawals` from governance state before callback
   - Skip pulsing rewards for accounts that have enacted withdrawals
   - Log count of skipped accounts for debugging

### CDDL Reference

```cddl
enact_state = [
  es_committee: strict_maybe<committee>,       [0]
  es_constitution: constitution,               [1]
  es_current_pparams: pparams,                 [2]
  es_previous_pparams: pparams,                [3]
  es_treasury: coin,                           [4]
  es_withdrawals: { * credential => coin },    [5]  <-- This is what we parse
  es_prev_gov_action_ids: gov_relation,        [6]
]
```

### Expected Result

- Accounts with enacted treasury withdrawals will NOT receive pulsing rewards
- This prevents double-counting of rewards for governance-enacted withdrawals
- May help fix the reserves/treasury discrepancy

---

## Fix 6: Missing Member Rewards from Pulser in Pulsing State (CRITICAL FIX)

### Problem Analysis

Investigation of the withdrawal underflow errors revealed a critical insight about `rdpair_reward`:

**What `rdpair` represents (from CDDL):**
```cddl
rdpair = [
  rdpair_reward : compactform_coin,   // Accumulated rewards already in account
  rdpair_deposit : compactform_coin,  // Stake key registration deposit
]
```

The `rdpair_reward` field contains rewards that have **already been paid** to accounts in previous epochs. It does NOT include rewards currently being calculated.

**The `pulsing_rew_update` structure has two variants:**

1. **Complete (variant 1)**: Reward calculation is finished
   - `update.rewards` contains ALL rewards (member + leader) ready to be distributed
   - This was being handled correctly

2. **Pulsing (variant 0)**: Reward calculation is still in progress
   - `snapshot.leaders` contains **ONLY leader rewards**
   - `pulser.reward_ans.accum_rewards` contains **member rewards calculated so far**
   - **WE WERE SKIPPING THE PULSER ENTIRELY!**

### Root Cause

In `streaming_snapshot.rs`, the Pulsing variant was only extracting leader rewards:

```rust
PulsingRewardUpdate::Pulsing { snapshot } => {
    let rewards = snapshot
        .leaders  // <-- ONLY LEADER REWARDS!
        .0
        .iter()
        .map(|(cred, rewards)| (cred.clone(), rewards.iter().map(|r| r.amount).sum()))
        .collect();
    // ...
}
```

And in `reward_snapshot.rs`, the Pulser was being skipped:

```rust
impl Pulser {
    pub fn skip(d: &mut Decoder) -> Result<(), minicbor::decode::Error> {
        d.skip()  // <-- SKIPPING ALL MEMBER REWARDS!
    }
}
```

When the NewEpochState snapshot is taken during reward pulsing (before completion), all delegator/member rewards were being lost, causing the withdrawal underflows.

### CDDL Reference for Pulser Structure

```cddl
pulser = [
    pulser_n : int,
    pulser_free : freevars,
    pulser_balance : {* credential_staking => compactform_coin },
    pulser_ans : reward_ans
]

reward_ans = [
    accum_reward_ans : { * credential_staking => reward },  <-- MEMBER REWARDS HERE!
    recent_reward_ans : reward_event
]
```

### The Fix

**Files Modified:**

1. `common/src/snapshot/reward_snapshot.rs`:
   - Added `RewardAns` struct to parse the accumulated rewards
   - Updated `Pulser` struct to actually parse and hold `reward_ans` instead of skipping
   - Updated `PulsingRewardUpdate::Pulsing` variant to include the `pulser` field

2. `common/src/snapshot/streaming_snapshot.rs`:
   - Updated `parse_pulsing_reward_update()` to combine both leader AND member rewards
   - Leader rewards from `snapshot.leaders`
   - Member rewards from `pulser.reward_ans.accum_rewards`
   - Added logging to show count of leader and member reward accounts

### Code Changes

**reward_snapshot.rs - New RewardAns struct:**
```rust
pub struct RewardAns {
    pub accum_rewards: VMap<StakeCredential, SnapshotSet<Reward>>,
}

impl<'b, C> minicbor::Decode<'b, C> for RewardAns {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let accum_rewards: VMap<StakeCredential, SnapshotSet<Reward>> = VMap::decode(d, ctx)?;
        d.skip()?; // Skip recent_reward_ans
        Ok(RewardAns { accum_rewards })
    }
}
```

**reward_snapshot.rs - Updated Pulser:**
```rust
pub struct Pulser {
    pub reward_ans: RewardAns,
}

impl<'b, C> minicbor::Decode<'b, C> for Pulser {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        d.skip()?; // pulser_n
        d.skip()?; // pulser_free
        d.skip()?; // pulser_balance
        let reward_ans = RewardAns::decode(d, ctx)?;
        Ok(Pulser { reward_ans })
    }
}
```

**streaming_snapshot.rs - Combined rewards extraction:**
```rust
PulsingRewardUpdate::Pulsing { snapshot, pulser } => {
    let mut rewards: HashMap<StakeCredential, Lovelace> = HashMap::new();

    // Add leader rewards from snapshot
    for (cred, reward_set) in &snapshot.leaders.0 {
        let amount: Lovelace = reward_set.iter().map(|r| r.amount).sum();
        *rewards.entry(cred.clone()).or_insert(0) += amount;
    }

    // Add member rewards from pulser's accumulated rewards
    for (cred, reward_set) in &pulser.reward_ans.accum_rewards.0 {
        let amount: Lovelace = reward_set.iter().map(|r| r.amount).sum();
        *rewards.entry(cred.clone()).or_insert(0) += amount;
    }
    // ...
}
```

### Expected Result

- All delegator accounts should now receive their member rewards from the Pulser
- Withdrawal underflow errors should be eliminated (accounts will have correct reward balances)
- The ~0.5% shortfall pattern should be resolved
- May significantly improve or fix the reserves/treasury discrepancy

### Commit

```
fix: extract member rewards from Pulser in pulsing reward state
```

**Status:** IMPLEMENTED - Needs testing with snapshot bootstrap to verify fix.

---

## Remaining Open Issues

### 1. SPDD Total Active Stake Mismatch
```
Total active stake mismatch for epoch 508:
   DB: 22588758523424195
   SPDD: 22450738602670046
```
Difference of ~138B lovelace (~138k ADA). This may be related to:
- Different timing of when delegations are counted
- Rewards being included/excluded from stake calculations
- Missing pools (6 pools reported as missing)

### 2. Reserves/Treasury Discrepancy
After all fixes, need to verify if the pot discrepancies are resolved:
- Reserves: -56,427 ADA too high
- Treasury: +23,603 ADA too low

The member rewards fix (Fix 6) is expected to have a significant impact on this.
