//! Acropolis AccountsState: rewards calculations
//!
//! This module implements the Shelley rewards calculation as specified in the
//! Shelley Ledger Formal Specification, particularly:
//!
//! - **Figure 46**: `maxPool` and `mkApparentPerformance` formulas
//! - **Figure 47**: `roperator` (leader rewards) and `rmember` (member rewards) formulas
//! - **Figure 48**: `rewardOnePool` - complete pool reward calculation
//! - **Errata 17.4**: Aggregating union (∪+) for accounts receiving both leader AND member rewards
//!
//! ## Key Implementation Notes
//!
//! ### Aggregating Union (Errata 17.4)
//! When an account receives both member rewards (as a delegator) AND leader rewards
//! (as the pool's reward account), these must be **summed together**, not kept separate.
//! This was a bug in the original Shelley implementation fixed at Allegra, where 64
//! stake addresses were reimbursed via MIR certificates.
//!
//! ### Pool Owner Exclusion (Figure 48)
//! Pool owners are excluded from member rewards (`hk ∉ poolOwners pool`) because their
//! stake contribution is already accounted for in the `roperator` formula via the `s/σ` term.

use acropolis_common::epoch_snapshot::{EpochSnapshot, SnapshotSPO};
use acropolis_common::{
    protocol_params::ShelleyParams, rational_number::RationalNumber, Lovelace, PoolId, RewardType,
    SPORewards, StakeAddress,
};
use anyhow::{bail, Result};
use bigdecimal::{BigDecimal, One, ToPrimitive, Zero};
use std::cmp::min;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Reward detail for a single account from a single pool.
///
/// Note: Per Errata 17.4, if an account receives both leader and member rewards
/// from the same pool (e.g., when the pool's reward account also delegates to the pool),
/// these are aggregated into a single `RewardDetail` with the combined amount.
/// The `rtype` in this case will be `Leader` since leader rewards take precedence.
#[derive(Debug, Clone)]
pub struct RewardDetail {
    /// The stake address receiving this reward
    pub account: StakeAddress,

    /// Type of reward (Leader, Member, or PoolRefund)
    /// When rewards are aggregated (Errata 17.4), Leader takes precedence
    pub rtype: RewardType,

    /// Reward amount in lovelace
    /// May be the sum of leader + member rewards if aggregated
    pub amount: Lovelace,

    /// The pool that generated this reward
    pub pool: PoolId,
}

/// Result of a rewards calculation
#[derive(Debug, Default, Clone)]
pub struct RewardsResult {
    /// Epoch these rewards were earned in (when blocks produced)
    pub epoch: u64,

    /// Total rewards paid to registered accounts
    pub total_paid: u64,

    /// Total leader rewards NOT calculated because SPO reward account wasn't registered
    /// Per Shelley spec (Figure 48, 51): These are filtered out and stay in reserves via Δr2
    /// NOTE: This is NOT unregRU - that's for accounts registered at calc but deregistered at application
    pub total_unpaid_leader_rewards: u64,

    /// Rewards to be paid
    pub rewards: BTreeMap<PoolId, Vec<RewardDetail>>,

    /// SPO rewards
    pub spo_rewards: Vec<(PoolId, SPORewards)>,
}

/// Calculate rewards for a given epoch based on current rewards state and protocol parameters
/// The epoch is the one that has just ended - we assume the snapshot for this has already been
/// taken.
/// Registrations/deregistrations are net changes between 'staking' and 'performance' snapshots
/// Note immutable - only state change allowed is to push a new snapshot
///
/// `registered_stake_addresses` is the set of currently registered stake addresses at calculation
/// time. Per Shelley spec (Figure 48), leader rewards are only paid if the SPO's reward account
/// is registered in the CURRENT dstate, not based on historical snapshot data.
pub fn calculate_rewards(
    epoch: u64,
    performance: Arc<EpochSnapshot>,
    staking: Arc<EpochSnapshot>,
    params: &ShelleyParams,
    stake_rewards: Lovelace,
    _registrations: &HashSet<StakeAddress>,
    deregistrations: &HashSet<StakeAddress>,
    registered_stake_addresses: &HashSet<StakeAddress>,
) -> Result<RewardsResult> {
    let mut result = RewardsResult {
        epoch,
        ..Default::default()
    };

    // If no blocks produced in previous epoch, don't do anything
    let total_blocks = performance.blocks;
    if total_blocks == 0 {
        return Ok(result);
    }

    // Take stake rewards from epoch we just left
    let stake_rewards = BigDecimal::from(stake_rewards);

    // Calculate total supply (total in circulation + treasury) or
    // equivalently (max-supply-reserves) - this is the denominator
    // for sigma, z0, s
    let total_supply = BigDecimal::from(params.max_lovelace_supply - performance.pots.reserves);

    // Get total active stake across all SPOs
    let total_active_stake =
        BigDecimal::from(staking.spos.values().map(|s| s.total_stake).sum::<Lovelace>());

    info!(epoch, go=staking.epoch, mark=performance.epoch,
          %total_supply, %total_active_stake, %stake_rewards, total_blocks,
          "Calculating rewards:");

    // Relative pool saturation size (z0)
    let k = BigDecimal::from(&params.protocol_params.stake_pool_target_num);
    if k.is_zero() {
        bail!("k is zero!");
    }
    let relative_pool_saturation_size = k.inverse();

    // Pledge influence factor (a0)
    let a0 = &params.protocol_params.pool_pledge_influence;
    let pledge_influence_factor = BigDecimal::from(a0.numer()) / BigDecimal::from(a0.denom());

    // Calculate for every registered SPO (even those who didn't participate in this epoch)
    // from epoch (i-2) "Go"
    let mut total_paid_to_pools: Lovelace = 0;
    let mut total_paid_to_delegators: Lovelace = 0;
    let mut num_pools_paid: usize = 0;
    let mut num_delegators_paid: usize = 0;
    let mut num_pools_with_blocks: usize = 0;
    let mut num_pools_registered: usize = 0;
    let mut num_pools_not_registered: usize = 0;

    info!(
        "Rewards calculation: {} SPOs in staking snapshot, {} registered addresses available",
        staking.spos.len(),
        registered_stake_addresses.len()
    );

    for (operator_id, staking_spo) in staking.spos.iter() {
        // Actual blocks produced for epoch i, no rewards if none
        let performance_spo = performance.spos.get(operator_id);
        let blocks_produced = performance_spo.map(|s| s.blocks_produced).unwrap_or(0);
        if blocks_produced == 0 {
            continue;
        }
        num_pools_with_blocks += 1;

        // Note: We no longer use performance_spo.two_previous_reward_account_is_registered
        // because the spec says to check current registration, not historical.
        let _performance_spo = performance_spo.unwrap();

        // Per Shelley spec (Figure 48): isRRegistered = rewardAcnt ∈ dom (rewards pp dstate)
        // The SPO's reward account must be registered in the CURRENT dstate (at calculation time),
        // NOT based on historical snapshot data from 2 epochs ago.
        //
        // We check the staking_spo.reward_account (the reward account as it was at stake snapshot
        // time) against the CURRENT registered addresses.
        let mut pay_to_pool_reward_account =
            registered_stake_addresses.contains(&staking_spo.reward_account);

        if pay_to_pool_reward_account {
            num_pools_registered += 1;
        } else {
            num_pools_not_registered += 1;
        }

        debug!(
            "SPO {} reward account {} registered now: {}",
            operator_id, staking_spo.reward_account, pay_to_pool_reward_account
        );

        // There was a bug in the original node from Shelley until Allegra where if multiple SPOs
        // shared a reward account, only one of them would get paid.
        // This was fixed at Allegra (epoch 236). After that, all pools with shared reward accounts
        // get their rewards aggregated together per Errata 17.4.
        const ALLEGRA_START_EPOCH: u64 = 236;
        if epoch < ALLEGRA_START_EPOCH && pay_to_pool_reward_account {
            // Pre-Allegra: simulate the bug where only lowest pool ID gets paid
            for (other_id, other_spo) in staking.spos.iter() {
                if other_spo.reward_account == staking_spo.reward_account
                    && other_id.cmp(operator_id) == Ordering::Less
                {
                    if performance.spos.get(other_id).map(|s| s.blocks_produced).unwrap_or(0) > 0 {
                        pay_to_pool_reward_account = false;
                        warn!("Shelley shared reward account bug (pre-Allegra): Dropping reward to {} in favour of {} on shared account {}",
                              operator_id,
                              other_id,
                              staking_spo.reward_account);
                        break;
                    }
                }
            }
        }

        if !pay_to_pool_reward_account {
            info!("Reward account for SPO {} isn't registered", operator_id)
        }

        // Calculate rewards for this SPO
        let (rewards, unpaid_leader_rewards) = calculate_spo_rewards(
            operator_id,
            staking_spo,
            blocks_produced as u64,
            total_blocks,
            &stake_rewards,
            &total_supply,
            &total_active_stake,
            &relative_pool_saturation_size,
            &pledge_influence_factor,
            params,
            staking.clone(),
            pay_to_pool_reward_account,
            deregistrations,
        );

        // Track leader rewards NOT calculated (SPO reward account not registered) - these stay in reserves
        result.total_unpaid_leader_rewards += unpaid_leader_rewards;

        if !rewards.is_empty() {
            let mut spo_rewards = SPORewards {
                total_rewards: 0,
                operator_rewards: 0,
            };
            for reward in &rewards {
                match reward.rtype {
                    RewardType::Leader => {
                        num_pools_paid += 1;
                        spo_rewards.operator_rewards += reward.amount;
                        total_paid_to_pools += reward.amount;
                    }
                    RewardType::Member => {
                        num_delegators_paid += 1;
                        total_paid_to_delegators += reward.amount;
                    }
                    RewardType::PoolRefund => {}
                }
                spo_rewards.total_rewards += reward.amount;
                result.total_paid += reward.amount;
            }

            result.rewards.insert(*operator_id, rewards);
            result.spo_rewards.push((*operator_id, spo_rewards));
        }
    }

    info!(
        num_pools_with_blocks,
        num_pools_registered,
        num_pools_not_registered,
        num_pools_paid,
        num_delegators_paid,
        total_paid_to_delegators,
        total_paid_to_pools,
        total_paid = result.total_paid,
        total_unpaid_leader_rewards = result.total_unpaid_leader_rewards,
        "Rewards calculated:"
    );

    Ok(result)
}

/// Calculate rewards for an individual SPO (stake pool operator).
///
/// Implements the `rewardOnePool` function from Shelley spec Figure 48.
///
/// ## Formulas Used
///
/// ### maxPool (Figure 46)
/// ```text
/// maxPool pp R σ pr = ⌊R/(1+a0) · (σ' + p'·a0·(σ' - p'·(z0-σ')/z0)/z0)⌋
/// ```
/// Where:
/// - R = total stake rewards available
/// - σ = relative pool stake (pool_stake / total_supply)
/// - σ' = min(σ, z0) (capped at saturation)
/// - pr = relative pledge (pledge / total_supply)
/// - p' = min(pr, z0)
/// - z0 = 1/k (saturation threshold)
/// - a0 = pledge influence factor
///
/// ### mkApparentPerformance (Figure 46)
/// ```text
/// mkApparentPerformance d σ n N = β/σ  if d < 0.8, else 1
/// where β = n/max(1,N)
/// ```
/// Where:
/// - d = decentralisation parameter
/// - n = blocks produced by this pool
/// - N = total blocks in epoch
/// - σ = relative active stake
///
/// ### roperator (Figure 47) - Leader Reward
/// ```text
/// roperator f̂ pool s σ = f̂                                  if f̂ ≤ c
///                       = c + ⌊(f̂-c)·(m + (1-m)·s/σ)⌋       otherwise
/// ```
///
/// ### rmember (Figure 47) - Member Reward
/// ```text
/// rmember f̂ pool t σ = 0                        if f̂ ≤ c
///                     = ⌊(f̂-c)·(1-m)·t/σ⌋       otherwise
/// ```
///
/// ## Aggregating Union (Errata 17.4)
///
/// Per Shelley spec Errata 17.4, when an account receives both member and leader
/// rewards (e.g., when the pool's reward account also delegates to the pool),
/// we must use an **aggregating union (∪+)** to sum the rewards together.
///
/// This was a bug in the original Shelley implementation where only one reward
/// type was kept. Fixed at Allegra hard fork with 64 affected addresses reimbursed.
/// Returns (rewards_to_pay, unpaid_leader_rewards)
/// The unpaid amount is for SPOs whose reward account is not registered at calculation time.
/// Per Shelley spec (Figure 48, 51): these rewards stay in RESERVES (via Δr2), not treasury.
/// Note: unregRU (treasury) is different - that's for accounts registered at calc but deregistered at application.
#[allow(clippy::too_many_arguments)]
fn calculate_spo_rewards(
    operator_id: &PoolId,
    spo: &SnapshotSPO,
    blocks_produced: u64,
    total_blocks: usize,
    stake_rewards: &BigDecimal,
    total_supply: &BigDecimal,
    total_active_stake: &BigDecimal,
    relative_pool_saturation_size: &BigDecimal,
    pledge_influence_factor: &BigDecimal,
    params: &ShelleyParams,
    staking: Arc<EpochSnapshot>,
    pay_to_pool_reward_account: bool,
    _deregistrations: &HashSet<StakeAddress>,
) -> (Vec<RewardDetail>, u64) {
    // =========================================================================
    // Step 1: Validate pool has stake and meets pledge
    // =========================================================================

    // Active stake (σ in the spec)
    let pool_stake = BigDecimal::from(spo.total_stake);
    if pool_stake.is_zero() {
        warn!("SPO {} has no stake - skipping", operator_id);
        return (vec![], 0);
    }

    // Get the stake actually delegated by the owner accounts to this SPO
    let pool_owner_stake =
        staking.get_stake_delegated_to_spo_by_addresses(operator_id, &spo.pool_owners);

    // Per Figure 48: maxP = 0 if pledge > ostake (pledge not met)
    if pool_owner_stake < spo.pledge {
        debug!(
            "SPO {} has owner stake {} less than pledge {} - skipping (per Figure 48: maxP=0)",
            operator_id, pool_owner_stake, spo.pledge
        );
        return (vec![], 0);
    }

    // =========================================================================
    // Step 2: Calculate maxPool (Figure 46)
    // =========================================================================

    let pool_pledge = BigDecimal::from(&spo.pledge);

    // σ = pool_stake / total_supply (relative pool stake)
    let relative_pool_stake = &pool_stake / total_supply;
    // σ' = min(σ, z0) where z0 = 1/k
    let capped_relative_pool_stake = min(&relative_pool_stake, relative_pool_saturation_size);

    // pr = pledge / total_supply (relative pledge)
    let relative_pool_pledge = &pool_pledge / total_supply;
    // p' = min(pr, z0)
    let capped_relative_pool_pledge = min(&relative_pool_pledge, relative_pool_saturation_size);

    // maxPool formula (Figure 46):
    // maxPool pp R σ pr = ⌊R/(1+a0) · (σ' + p'·a0·(σ' - p'·(z0-σ')/z0)/z0)⌋
    let optimum_rewards = ((stake_rewards / (BigDecimal::one() + pledge_influence_factor))
        * (capped_relative_pool_stake
            + (capped_relative_pool_pledge
                * pledge_influence_factor
                * (capped_relative_pool_stake
                    - (capped_relative_pool_pledge
                        * ((relative_pool_saturation_size - capped_relative_pool_stake)
                            / relative_pool_saturation_size))))
                / relative_pool_saturation_size))
        .with_scale(0);

    // =========================================================================
    // Step 3: Calculate mkApparentPerformance (Figure 46)
    // =========================================================================

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
            / BigDecimal::from(total_blocks as u64);

        debug!(blocks_produced, %relative_blocks, %pool_stake, %relative_active_stake,
               "Pool performance calc (mkApparentPerformance):");
        &relative_blocks / &relative_active_stake
    };

    // =========================================================================
    // Step 4: Calculate actual pool reward (poolR = ⌊appPerf · maxP⌋)
    // =========================================================================

    let pool_rewards = (&optimum_rewards * &pool_performance).with_scale(0);

    debug!(%pool_stake, %relative_pool_stake, %pool_performance,
           %optimum_rewards, %pool_rewards, pool_owner_stake, %pool_pledge,
           "Pool {} reward calculation:", operator_id);

    // =========================================================================
    // Step 5: Calculate leader reward - roperator (Figure 47)
    // =========================================================================

    let fixed_cost = BigDecimal::from(spo.fixed_cost);

    // Use a HashMap to aggregate rewards by account (Errata 17.4 - aggregating union ∪+)
    // Key: StakeAddress, Value: (accumulated_amount, is_leader_reward)
    let mut reward_aggregator: HashMap<StakeAddress, (u64, bool)> = HashMap::new();

    let spo_benefit = if pool_rewards <= fixed_cost {
        // roperator f̂ pool s σ = f̂ when f̂ ≤ c
        debug!("Rewards {} <= cost {} - all paid to SPO (per roperator formula)", pool_rewards, fixed_cost);
        pool_rewards.to_u64().unwrap_or(0)
    } else {
        // roperator f̂ pool s σ = c + ⌊(f̂-c)·(m + (1-m)·s/σ)⌋ when f̂ > c
        let margin =
            BigDecimal::from(spo.margin.numerator) / BigDecimal::from(spo.margin.denominator);

        // s/σ = owner_stake/pool_stake (relative owner stake within pool context)
        // But per spec, both s and σ are relative to total_supply, so:
        // s = owner_stake/total_supply, σ = pool_stake/total_supply
        // s/σ = owner_stake/pool_stake (the total_supply cancels out)
        let relative_owner_stake = pool_owner_stake / total_supply;
        let margin_cost = ((&pool_rewards - &fixed_cost)
            * (&margin
                + (BigDecimal::one() - &margin) * (relative_owner_stake / &relative_pool_stake)))
            .with_scale(0);
        let leader_reward = &fixed_cost + &margin_cost;

        // =====================================================================
        // Step 6: Calculate member rewards - rmember (Figure 47)
        // =====================================================================

        // rmember f̂ pool t σ = ⌊(f̂-c)·(1-m)·t/σ⌋ when f̂ > c
        // Note: The spec recalculates without the owner stake term for member rewards
        let to_delegators = (&pool_rewards - &fixed_cost) * (BigDecimal::one() - &margin);
        let mut total_member_paid: u64 = 0;
        let mut delegators_paid: usize = 0;

        if !to_delegators.is_zero() {
            let total_stake = BigDecimal::from(spo.total_stake);

            for (delegator_stake_address, stake) in &spo.delegators {
                // t/σ = delegator_stake / pool_stake (proportion of pool)
                let proportion = BigDecimal::from(stake) / &total_stake;
                let reward = &to_delegators * &proportion;
                let to_pay = reward.with_scale(0).to_u64().unwrap_or(0);

                // Skip if rounded to zero
                if to_pay == 0 {
                    continue;
                }

                debug!(
                    "Member reward: stake {} -> proportion {} of {} -> {} to {}",
                    stake, proportion, to_delegators, to_pay, delegator_stake_address
                );

                // Per Figure 48: mRewards excludes pool owners (hk ∉ poolOwners pool)
                // Pool owners get their share through the roperator formula's s/σ term
                if spo.pool_owners.contains(delegator_stake_address) {
                    debug!(
                        "Skipping pool owner {} from member rewards (per Figure 48: hk ∉ poolOwners)",
                        delegator_stake_address
                    );
                    continue;
                }

                // NOTE: We do NOT exclude the pool reward account from member rewards!
                // The spec (Figure 48) only excludes pool owners (poolOwners pool).
                // If the pool reward account delegates to this pool and is not an owner,
                // they ARE entitled to member rewards, which will be aggregated with
                // their leader rewards per Errata 17.4.

                // NOTE: We do NOT filter out deregistered accounts here.
                // Per Shelley spec (Figure 52 - applyRUpd), rewards for unregistered accounts
                // should go to treasury, not be silently dropped. The application phase in
                // complete_previous_epoch_rewards_calculation handles this correctly.

                // Add to aggregator (Errata 17.4 - aggregating union ∪+)
                let entry = reward_aggregator
                    .entry(delegator_stake_address.clone())
                    .or_insert((0, false));
                entry.0 += to_pay;
                // Keep track if this is also a leader reward (will be set below if applicable)

                total_member_paid += to_pay;
                delegators_paid += 1;
            }
        }

        debug!(
            %fixed_cost, %margin_cost, leader_reward=%leader_reward, %to_delegators,
            total_member_paid, delegators_paid,
            "Reward split (roperator + rmember):"
        );

        leader_reward.to_u64().unwrap_or(0)
    };

    // =========================================================================
    // Step 7: Add leader reward to aggregator (may aggregate with member reward)
    // =========================================================================

    if pay_to_pool_reward_account {
        // Per Errata 17.4: If the pool reward account already received member rewards
        // (because it delegates to this pool), we aggregate (∪+) the amounts together.
        let entry = reward_aggregator
            .entry(spo.reward_account.clone())
            .or_insert((0, false));

        if entry.0 > 0 {
            // This account already has member rewards - aggregate them!
            info!(
                "SPO {} reward account {} receives both leader ({}) AND member ({}) rewards - aggregating per Errata 17.4",
                operator_id, spo.reward_account, spo_benefit, entry.0
            );
        }

        entry.0 += spo_benefit;
        entry.1 = true; // Mark as having leader reward (for RewardType)
    }

    // Track leader rewards NOT calculated because reward account wasn't registered
    // Per Shelley spec (Figure 48, 51):
    // - rewards = addrsrew ⨃ potentialRewards (filtered to registered accounts only)
    // - Δr2 = R - sum(rs) (leftover stays in reserves)
    // So these rewards are NOT calculated and stay in reserves, not treasury!
    // (Only accounts registered at calc but deregistered at application go to treasury - that's unregRU)
    let unpaid_leader_rewards = if !pay_to_pool_reward_account {
        info!(
            "SPO {}'s reward account {} leader reward {} NOT calculated (account not registered) - stays in reserves",
            operator_id, spo.reward_account, spo_benefit,
        );
        spo_benefit
    } else {
        0
    };

    // =========================================================================
    // Step 8: Convert aggregator to final rewards vector
    // =========================================================================

    let rewards: Vec<RewardDetail> = reward_aggregator
        .into_iter()
        .filter(|(_, (amount, _))| *amount > 0)
        .map(|(account, (amount, is_leader))| RewardDetail {
            account,
            // If this account received leader rewards, mark as Leader (takes precedence)
            rtype: if is_leader {
                RewardType::Leader
            } else {
                RewardType::Member
            },
            amount,
            pool: *operator_id,
        })
        .collect();

    (rewards, unpaid_leader_rewards)
}
