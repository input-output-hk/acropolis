# Logs to Watch - Next Run

## Critical: Bootstrap to Live Transition

These logs appear at the first epoch boundary after bootstrap (epoch 507 → 508):

```
[BOOTSTRAP->LIVE TRANSITION] First epoch boundary after bootstrap (bootstrap epoch 507)
[BOOTSTRAP->LIVE TRANSITION] Creating live snapshot for epoch 507 to replace bootstrap Mark
```

## Critical: Snapshot Comparison

If there are discrepancies between bootstrap and live-calculated snapshots, you'll see:

```
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: Total stake differs by X lovelace (Y ADA) - old=..., new=...
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: Pool count differs - old=X, new=Y
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: X pools in OLD but not NEW: [...]
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: X pools in NEW but not OLD: [...]
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: X pools with stake differences, total diff=Y lovelace (Z ADA)
[SNAPSHOT COMPARE] Bootstrap Mark vs Live Mark: Pool <pool_id> stake diff X (Y ADA): old=..., new=...
```

**This is the key diagnostic** - it will show exactly which pools have stake differences and by how much.

## Critical: Pulsing Rewards Validation

```
[PULSING VALIDATION] Rewards complete: extracted=X matches expected=Y
[PULSING VALIDATION] Rewards difference within tolerance: extracted=X, expected=Y, diff=Z (P%)
[PULSING VALIDATION WARN] Rewards may be INCOMPLETE: extracted=X (Y ADA), expected=Z (W ADA), diff=... (P%)
```

If you see the WARN message with >1% difference, pulsing rewards may be incomplete.

## New: Aggregating Union (Errata 17.4)

When an SPO's reward account also delegates to their pool, you'll now see:

```
SPO <pool_id> reward account <stake_addr> receives both leader (X) AND member (Y) rewards - aggregating per Errata 17.4
```

This is the fix for the dual rewards issue - confirms aggregation is working.

## Bootstrap Protocol Parameters

When protocol parameters are received at bootstrap, you'll see the previous epoch's reward params:

```
Received governance protocol parameters for epoch 507 (previous_reward_params: k=..., a0=..., rho=..., tau=..., min_pool_cost=...)
```

When the first protocol parameters message is handled, confirming previous params are set:

```
Bootstrap: setting previous_protocol_parameters to match current
```

## Bootstrap Snapshot Summary

At bootstrap, these show what was loaded:

```
[BOOTSTRAP SNAPSHOT] Mark (epoch 506): X SPOs, Y total blocks, reserves=..., treasury=..., deposits=...
[BOOTSTRAP SNAPSHOT] Set (epoch 505): X SPOs, Y total blocks
[BOOTSTRAP SNAPSHOT] Go (epoch 504): X SPOs, Y total blocks
```

## Live Snapshot Summary

When the first live snapshot is created:

```
[LIVE SNAPSHOT] New Mark (epoch 507): X SPOs, total_stake=Y (Z ADA), W blocks
```

## Reward Calculation Details

For detailed reward calculation (debug level):

```
Pool <id> reward calculation: pool_stake=..., relative_pool_stake=..., pool_performance=..., optimum_rewards=..., pool_rewards=...
Reward split (roperator + rmember): fixed_cost=..., margin_cost=..., leader_reward=..., to_delegators=..., total_member_paid=..., delegators_paid=...
Member reward: stake X -> proportion Y of Z -> W to <stake_addr>
Skipping pool owner <addr> from member rewards (per Figure 48: hk ∉ poolOwners)
```

## Pot Verification

```
Entering epoch X: reserves=..., treasury=...
After monetary change epoch X: reserves=..., treasury=...
```

## Error Conditions

Watch for these error/warning patterns:

```
[PULSING VALIDATION WARN] Rewards may be INCOMPLETE
Reward account for SPO <id> isn't registered
SPO <id>'s reward account <addr> not paid X (reward account not registered per Figure 48)
```

---

## Quick Grep Commands

```bash
# All snapshot comparison logs
grep "\[SNAPSHOT COMPARE\]" log_file.log

# All pulsing validation logs
grep "\[PULSING VALIDATION\]" log_file.log

# All bootstrap transition logs
grep "\[BOOTSTRAP" log_file.log

# Aggregating union occurrences (Errata 17.4 fix)
grep "aggregating per Errata 17.4" log_file.log

# Any warnings
grep -i "warn" log_file.log
```
