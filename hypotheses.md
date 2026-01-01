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

### Fix 2: DRep registration/deregistration deposit handling

**Problem Identified:**
The `handle_tx_certificates()` function in `accounts_state` was NOT handling `DRepRegistration` and `DRepDeregistration` certificates. These were falling through to the `_ => ()` catch-all case.

DRep registration requires a 500 ADA deposit, and deregistration refunds it. Without handling these, the deposits pot would drift over time as DReps register and deregister.

**The Fix:**
Added handling for `TxCertificate::DRepRegistration` and `TxCertificate::DRepDeregistration` in `handle_tx_certificates()`:
- Registration: `self.pots.deposits += reg.deposit`
- Deregistration: `self.pots.deposits -= dereg.refund`

**Files Modified:**
- `modules/accounts_state/src/state.rs` - Added DRep deposit handling in `handle_tx_certificates()`

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
