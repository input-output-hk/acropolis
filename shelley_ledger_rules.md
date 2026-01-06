# Shelley Ledger Formal Specification (SL-D5)

This document summarizes the key concepts from "A Formal Specification of the Cardano Ledger" (Deliverable SL-D5), focusing on rewards and epoch boundary transitions.

## Table of Contents

1. [Overview](#overview)
2. [Key Types](#key-types)
3. [Reward Cycle Timeline](#reward-cycle-timeline)
4. [Reward Calculation Functions](#reward-calculation-functions)
5. [Stake Distribution](#stake-distribution)
6. [Snapshots (Mark, Set, Go)](#snapshots-mark-set-go)
7. [Pool Reaping](#pool-reaping)
8. [Protocol Parameter Updates](#protocol-parameter-updates)
9. [Preservation of Value](#preservation-of-value)

---

## Overview

The Shelley ledger specification defines how rewards are calculated and distributed at epoch boundaries. The system uses a rotating snapshot mechanism ("mark, set, go") to ensure stable stake distributions for leader election and reward calculation.

---

## Key Types

### Protocol Parameters

| Parameter | Description |
|-----------|-------------|
| `a0` | Pledge influence factor (affects pool rewards based on pledge) |
| `n_opt` | Optimal number of saturated stake pools |
| `tau` (τ) | Treasury expansion rate (fraction of rewards going to treasury) |
| `rho` (ρ) | Monetary expansion rate (fraction of reserves released per epoch) |
| `d` | Decentralization parameter (0 = fully decentralized, 1 = federated) |
| `poolDeposit` | Deposit required for pool registration |
| `keyDeposit` | Deposit required for stake key registration |

### Accounting Fields (`Acnt`)

```
Acnt = {
    treasury: Coin,   -- Treasury pot
    reserves: Coin    -- Reserve pot (used to pay rewards)
}
```

### Snapshot Type

```
Snapshot = {
    stake: Credential -> Coin,           -- Stake distribution
    delegations: Credential -> KeyHash,  -- Delegation map
    poolParameters: KeyHash -> PoolParam -- Pool parameters
}
```

### Snapshots Container

```
Snapshots = {
    pstakeMark: Snapshot,  -- Newest snapshot
    pstakeSet: Snapshot,   -- Middle snapshot
    pstakeGo: Snapshot,    -- Oldest snapshot (used for rewards)
    feeSS: Coin            -- Fee snapshot
}
```

### Reward Update

```
RewardUpdate = {
    deltaT: Coin,              -- Change to treasury (positive)
    deltaR: Coin,              -- Change to reserves (negative)
    rs: AddrRwd -> Coin,       -- New individual rewards map
    deltaF: Coin               -- Change to fee pot (negative)
}
```

---

## Reward Cycle Timeline

For rewards in epoch `e_i`, the cycle involves epochs `e_{i-1}`, `e_i`, and `e_{i+1}`:

```
e_{i-1}          e_i              e_{i+1}
   |              |                  |
   A    B    C    D         E    F   G
   |----|----|----|---------|----|---|
```

| Point | Event |
|-------|-------|
| A | Stake distribution snapshot taken at beginning of `e_{i-1}` |
| B | Randomness for leader election fixed during `e_{i-1}` |
| C | Epoch `e_i` begins |
| D | Epoch `e_i` ends; snapshot of pool performance and fee pot taken |
| E | Snapshots stable; reward calculation can begin (2k blocks after D) |
| F | Reward calculation finished; update ready to apply |
| G | Rewards distributed |

**Key Insight**: Rewards for epoch `e_i` use the stake snapshot from `e_{i-1}` and are paid out in `e_{i+1}`.

---

## Reward Calculation Functions

### 1. `maxPool` - Maximum Pool Reward

Calculates the maximum reward a pool can receive based on stake and pledge.

```
maxPool(pp, R, σ, p_r) = R / (1 + a0) * (σ' + p' * a0 * (σ' - p' * (z0 - σ') / z0) / z0)

where:
    a0 = influence(pp)           -- Pledge influence
    n_opt = nOpt(pp)             -- Optimal pool count
    z0 = 1 / n_opt               -- Saturation threshold
    σ' = min(σ, z0)              -- Capped relative stake
    p' = min(p_r, z0)            -- Capped relative pledge
```

**Parameters**:
- `R`: Total rewards available for the epoch
- `σ`: Pool's relative stake (pool_stake / total_stake)
- `p_r`: Pool's relative pledge (pledge / total_stake)

### 2. `mkApparentPerformance` - Pool Performance

Calculates apparent performance based on blocks produced.

```
mkApparentPerformance(d, σ, n, N) =
    if d < 0.8:
        β / σ     where β = n / max(1, N)
    else:
        1
```

**Parameters**:
- `d`: Decentralization parameter
- `σ`: Pool's relative active stake
- `n`: Blocks produced by pool
- `N`: Total blocks produced in epoch

**Note**: When `d >= 0.8` (mostly federated), performance is always 1.

### 3. `r_operator` - Leader/Operator Reward

Calculates the pool operator's reward.

```
r_operator(f_hat, pool, s, σ) =
    if f_hat <= c:
        f_hat
    else:
        c + (f_hat - c) * (m + (1 - m) * s / σ)

where:
    c = poolCost(pool)     -- Fixed cost
    m = poolMargin(pool)   -- Margin (0 to 1)
    s = owner stake        -- Total stake of pool owners
    σ = pool's total relative stake
```

**Logic**: Operator gets the cost first, then margin on remainder, plus proportional share of what's left based on owner stake.

### 4. `r_member` - Member Reward

Calculates a delegator's reward.

```
r_member(f_hat, pool, t, σ) =
    if f_hat <= c:
        0
    else:
        (f_hat - c) * (1 - m) * t / σ

where:
    c = poolCost(pool)
    m = poolMargin(pool)
    t = member's stake
    σ = pool's total relative stake
```

**Logic**: Members get nothing if pool reward doesn't exceed cost. Otherwise, they get proportional share of (reward - cost) * (1 - margin).

### 5. `rewardOnePool` - Single Pool Rewards

Combines all the above to calculate rewards for one pool:

```
rewardOnePool(pp, R, n, N, pool, stake, σ, σ_a, total, addrsRew) = rewards

where:
    ostake = sum of stake for pool owners
    pledge = poolPledge(pool)
    p_r = pledge / total

    maxP =
        maxPool(pp, R, σ, p_r)   if pledge <= ostake
        0                         otherwise (pledge not met!)

    appPerf = mkApparentPerformance(d(pp), σ_a, n, N)
    poolR = floor(appPerf * maxP)

    mRewards = {addr -> r_member(poolR, pool, stake, σ) for each member}
    lReward = r_operator(poolR, pool, ostake, σ)

    potentialRewards = mRewards ∪ {poolRAcnt(pool) -> lReward}
    rewards = addrsRew ◁ potentialRewards  -- Filter to registered accounts
```

**Critical**: If pledge is not met (`pledge > ostake`), the pool gets **zero rewards**.

### 6. `reward` - All Pools

Applies `rewardOnePool` to every registered pool and combines results.

---

## Stake Distribution

### `stakeDistr` Function

Calculates stake distribution from UTxO set and delegation state:

```
stakeDistr(utxo, dstate, pstate) = (activeDelegs ◁ aggregate(stakeRelation), delegations, poolParams)

where:
    stakeRelation = stakeCred_b^{-1} ∪ (addrPtr ∘ ptr)^{-1} ∘ (range utxo) ∪ rewards
    activeDelegs = (dom rewards) ◁ delegations ▷ (dom poolParams)
```

**Sources of stake**:
1. Base addresses (via `stakeCred_b`)
2. Pointer addresses (via `addrPtr`)
3. Reward account balances

Only credentials that are both registered (in rewards) and delegated to a registered pool are included.

---

## Snapshots (Mark, Set, Go)

The system maintains three rotating snapshots for stability:

| Snapshot | Age | Purpose |
|----------|-----|---------|
| **Mark** | Newest | Just taken at current epoch boundary |
| **Set** | Middle | Used for leader election in current epoch |
| **Go** | Oldest | Used for reward calculation |

### SNAP Transition Rule

At each epoch boundary:
```
New state:
    pstakeMark = stakeDistr(utxo, dstate, pstate)  -- Fresh calculation
    pstakeSet = old.pstakeMark                      -- Mark becomes Set
    pstakeGo = old.pstakeSet                        -- Set becomes Go
    feeSS = fees                                    -- Capture current fees
```

---

## Pool Reaping

The POOLREAP transition handles pool retirement:

1. **Identify retired pools**: `retired = dom(retiring^{-1}(e))` for current epoch `e`

2. **Calculate refunds**: Each retiring pool gets `poolDeposit` back

3. **Distribute refunds**:
   - If reward account still registered: add to that account
   - If reward account deregistered: add to treasury

4. **Clean up state**:
   - Remove delegations to retired pools
   - Remove pools from `poolParams`, `fPoolParams`, `retiring`
   - Reduce deposit pot by total refunds

---

## Protocol Parameter Updates

The NEWPP transition handles protocol parameter changes:

**Acceptance conditions**:
1. New parameters must not cause debt exceeding reserves
2. `maxTxSize + maxHeaderSize < maxBlockSize`

**If accepted**:
- Update protocol parameters
- Adjust reserves based on deposit obligation change
- Reset update proposals

**If rejected**:
- Keep old parameters
- Only reset update proposals

---

## Preservation of Value

The system maintains a fundamental invariant: **total lovelace is constant**.

```
Circulation + Deposits + Fees + Reserves + Treasury + Rewards = CONSTANT
```

### Fund Flow Diagram

```
                    ┌──────────────┐
                    │  Circulation │ (UTxO)
                    └──────┬───────┘
                           │ deposits/refunds
                           ▼
                    ┌──────────────┐
                    │   Deposits   │
                    └──────┬───────┘
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
        ▼                  ▼                  ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│     Fees     │   │   Reserves   │   │   Treasury   │
└──────┬───────┘   └──────┬───────┘   └──────────────┘
        │                  │                  ▲
        │      (ρ expansion)                  │
        │                  │           (τ fraction)
        └─────────►┌───────▼───────┐◄─────────┘
                   │  Reward Pot   │ (temporary)
                   └───────┬───────┘
                           │
                           ▼
                   ┌──────────────┐
                   │Reward Accounts│
                   └──────────────┘
```

---

## createRUpd - Create Reward Update

```
createRUpd(slotsPerEpoch, b, es, total) = (deltaT, deltaR, rs, deltaF)

where:
    -- Use PREVIOUS protocol parameters (for the epoch being rewarded)
    prevPp = es.prevPp

    -- Monetary expansion from reserves
    η = if d(prevPp) >= 0.8 then 1
        else blocksMade / floor((1 - d) * slotsPerEpoch * activeSlotCoeff)

    deltaR1 = floor(min(1, η) * ρ(prevPp) * reserves)

    -- Total reward pot
    rewardPot = feeSS + deltaR1

    -- Treasury cut
    deltaT1 = floor(τ(prevPp) * rewardPot)
    R = rewardPot - deltaT1

    -- Calculate individual rewards
    circulation = total - reserves
    rs = reward(prevPp, b, R, dom(rewards), poolParams, stake, delegs, circulation)

    -- Unclaimed rewards go back to reserves
    deltaR2 = R - sum(rs)

    -- Final values
    deltaT = deltaT1
    deltaR = -deltaR1 + deltaR2  -- Net change (usually negative)
    deltaF = -feeSS
```

---

## applyRUpd - Apply Reward Update

```
applyRUpd(rewardUpdate, epochState) = newEpochState

where:
    -- Separate registered vs deregistered reward accounts
    regRU = (dom rewards) ◁ rs       -- Still registered
    unregRU = (dom rewards) ⊳ rs     -- No longer registered

    -- Update state
    treasury' = treasury + deltaT + sum(unregRU)  -- Unclaimed go to treasury
    reserves' = reserves + deltaR
    rewards' = rewards ∪+ regRU                   -- Add to existing balances
    fees' = fees + deltaF
```

**Note**: Rewards for deregistered accounts go to treasury, not lost.

---

## Key Implementation Considerations

1. **Pledge enforcement**: Pools not meeting pledge get zero rewards
2. **Performance calculation**: Only matters when `d < 0.8`
3. **Snapshot timing**: Use "go" snapshot (2 epochs old) for reward calculation
4. **Previous parameters**: Always use `prevPp` for reward calculations
5. **Registered accounts only**: Filter rewards to currently registered accounts
6. **Unclaimed rewards**: Go to treasury (from deregistered accounts) or reserves (unused pool rewards)
7. **Fee snapshot**: Use `feeSS` from snapshot, not current `fees`
