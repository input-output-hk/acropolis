//! Acropolis AccountsState: rewards calculations

use crate::snapshot::{Snapshot, SnapshotSPO};
use acropolis_common::{
    protocol_params::ShelleyParams, rational_number::RationalNumber, KeyHash, Lovelace,
    RewardAccount, SPORewards,
};
use anyhow::{bail, Result};
use bigdecimal::{BigDecimal, One, ToPrimitive, Zero};
use std::cmp::min;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Type of reward being given
#[derive(Debug, Clone)]
pub enum RewardType {
    Leader,
    Member,
}

/// Reward Detail
#[derive(Debug, Clone)]
pub struct RewardDetail {
    /// Account reward paid to
    pub account: RewardAccount,

    /// Type of reward
    pub rtype: RewardType,

    /// Reward amount
    pub amount: Lovelace,
}

/// Result of a rewards calculation
#[derive(Debug, Default)]
pub struct RewardsResult {
    /// Epoch these rewards were earned in (when blocks produced)
    pub epoch: u64,

    /// Total rewards paid
    pub total_paid: u64,

    /// Rewards to be paid
    pub rewards: BTreeMap<KeyHash, Vec<RewardDetail>>,

    /// SPO rewards
    pub spo_rewards: Vec<(KeyHash, SPORewards)>,
}

/// Calculate rewards for a given epoch based on current rewards state and protocol parameters
/// The epoch is the one that has just ended - we assume the snapshot for this has already been
/// taken.
/// Note immutable - only state change allowed is to push a new snapshot
pub fn calculate_rewards(
    epoch: u64,
    performance: Arc<Snapshot>,
    staking: Arc<Snapshot>,
    params: &ShelleyParams,
    stake_rewards: Lovelace,
    previous_epoch_deregistrations: &HashSet<KeyHash>,
) -> Result<RewardsResult> {
    let mut result = RewardsResult::default();
    result.epoch = epoch;

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
    for (operator_id, spo) in staking.spos.iter() {
        // Actual blocks produced for epoch i, no rewards if none
        let performance_spo = performance.spos.get(operator_id);
        let blocks_produced = performance_spo.map(|s| s.blocks_produced).unwrap_or(0);
        if blocks_produced == 0 {
            continue;
        }

        // SPO's reward account from staking time must be registered now
        // We get the registration status *as it is in performance* for the reward account
        // *as it was during staking*
        let mut pay_to_pool_reward_account =
            performance_spo.map(|s| s.two_previous_reward_account_is_registered).unwrap_or(false);

        // There was a bug in the original node from Shelley until Allegra where if multiple SPOs
        // shared a reward account, only one of them would get paid.
        // QUESTION: Which one?  Lowest hash seems to work in epoch 212
        // TODO turn this off at Allegra start
        if pay_to_pool_reward_account {
            // Check all SPOs to see if they match this reward account
            for (other_id, other_spo) in staking.spos.iter() {
                if other_spo.reward_account == spo.reward_account
                    && other_id.cmp(&operator_id) == Ordering::Less
                // Lower ID (hash) wins
                {
                    // It must have been paid a reward - we assume that checking it produced
                    // any blocks is enough here - if not we'll have to do this as a post-process
                    if performance.spos.get(other_id).map(|s| s.blocks_produced).unwrap_or(0) > 0 {
                        pay_to_pool_reward_account = false;
                        warn!("Shelley shared reward account bug: Dropping reward to {} in favour of {} on shared account {}",
                              hex::encode(&operator_id),
                              hex::encode(&other_id),
                              hex::encode(&spo.reward_account));
                        break;
                    }
                }
            }
        } else {
            info!(
                "Reward account for SPO {} was deregistered",
                hex::encode(operator_id)
            )
        }

        // Calculate rewards for this SPO
        let rewards = calculate_spo_rewards(
            operator_id,
            spo,
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
            previous_epoch_deregistrations,
        );

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
                }
                spo_rewards.total_rewards += reward.amount;
                result.total_paid += reward.amount;
            }

            result.rewards.insert(operator_id.clone(), rewards);
            result.spo_rewards.push((operator_id.clone(), spo_rewards));
        }
    }

    info!(
        num_delegators_paid,
        total_paid_to_delegators,
        num_pools_paid,
        total_paid_to_pools,
        total = result.total_paid,
        "Rewards actually paid:"
    );

    Ok(result)
}

/// Calculate rewards for an individual SPO
fn calculate_spo_rewards(
    operator_id: &KeyHash,
    spo: &SnapshotSPO,
    blocks_produced: u64,
    total_blocks: usize,
    stake_rewards: &BigDecimal,
    total_supply: &BigDecimal,
    total_active_stake: &BigDecimal,
    relative_pool_saturation_size: &BigDecimal,
    pledge_influence_factor: &BigDecimal,
    params: &ShelleyParams,
    staking: Arc<Snapshot>,
    pay_to_pool_reward_account: bool,
    deregistrations: &HashSet<KeyHash>,
) -> Vec<RewardDetail> {

    // Active stake (sigma)
    let pool_stake = BigDecimal::from(spo.total_stake);
    if pool_stake.is_zero() {
        warn!(
            "SPO {} has no stake - skipping",
            hex::encode(&operator_id),
        );

        // No stake, no rewards or earnings
        return vec![];
    }

    // Get the stake actually delegated by the owners accounts to this SPO
    let pool_owner_stake =
        staking.get_stake_delegated_to_spo_by_addresses(&operator_id, &spo.pool_owners);

    // If they haven't met their pledge, no dice
    if pool_owner_stake < spo.pledge {
        debug!(
            "SPO {} has owner stake {} less than pledge {} - skipping",
            hex::encode(&operator_id),
            pool_owner_stake,
            spo.pledge
        );
        return vec![];
    }

    let pool_pledge = BigDecimal::from(&spo.pledge);

    // Relative stake as fraction of total supply (sigma), and capped with 1/k (sigma')
    let relative_pool_stake = &pool_stake / total_supply;
    let capped_relative_pool_stake = min(&relative_pool_stake, &relative_pool_saturation_size);

    // Stake pledged by operator (s) and capped with 1/k (s')
    let relative_pool_pledge = &pool_pledge / total_supply;
    let capped_relative_pool_pledge = min(&relative_pool_pledge, &relative_pool_saturation_size);

    // Get the optimum reward for this pool
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

    // If decentralisation_param >= 0.8 => performance = 1
    // Shelley Delegation Spec 3.8.3
    let decentralisation = &params.protocol_params.decentralisation_param;
    let pool_performance = if decentralisation >= &RationalNumber::new(8, 10) {
        BigDecimal::one()
    } else {
        let relative_active_stake = &pool_stake / total_active_stake;
        let relative_blocks = BigDecimal::from(blocks_produced)  // Beta
            / BigDecimal::from(total_blocks as u64);

        info!(blocks_produced, %relative_blocks, %pool_stake, %relative_active_stake,
              "Pool performance calc:");
        &relative_blocks / &relative_active_stake
    };

    // Get actual pool rewards
    let pool_rewards = (&optimum_rewards * &pool_performance).with_scale(0);

    info!(%pool_stake, %relative_pool_stake, %pool_performance,
          %optimum_rewards, %pool_rewards, pool_owner_stake, %pool_pledge,
           "Pool {}", hex::encode(&operator_id));

    // Subtract fixed costs
    let fixed_cost = BigDecimal::from(spo.fixed_cost);
    let mut rewards = Vec::<RewardDetail>::new();
    let spo_benefit = if pool_rewards <= fixed_cost {
        debug!("Rewards < cost - all paid to SPO");

        // No margin or pledge reward if under cost - all goes to SPO
        pool_rewards.to_u64().unwrap_or(0)
    } else {
        // Enough left over for some margin split
        let margin =
            BigDecimal::from(spo.margin.numerator) / BigDecimal::from(spo.margin.denominator);

        let relative_owner_stake = &pool_owner_stake / total_supply;
        let margin_cost = ((&pool_rewards - &fixed_cost)
            * (&margin
                + (BigDecimal::one() - &margin) * (relative_owner_stake / relative_pool_stake)))
            .with_scale(0);
        let costs = &fixed_cost + &margin_cost;

        // Pay the delegators - split remainder in proportional to delegated stake,
        // * as it was 2 epochs ago *

        // You'd think this was just (pool_rewards - costs) here, but the Haskell code recalculates
        // the margin without the relative_owner_stake term !?
        // Note keeping fractional part, which is non-obvious
        let to_delegators = (&pool_rewards - &fixed_cost) * (BigDecimal::one() - &margin);
        let mut total_paid: u64 = 0;
        let mut delegators_paid: usize = 0;
        if !to_delegators.is_zero() {
            let total_stake = BigDecimal::from(spo.total_stake);
            for (hash, stake) in &spo.delegators {
                let proportion = BigDecimal::from(stake) / &total_stake;

                // and hence how much of the total reward they get
                let reward = &to_delegators * &proportion;
                let to_pay = reward.with_scale(0).to_u64().unwrap_or(0);

                debug!("Reward stake {stake} -> proportion {proportion} of SPO rewards {to_delegators} -> {to_pay} to hash {}",
                       hex::encode(hash));

                // Pool owners don't get member rewards (seems unfair!)
                if spo.pool_owners.contains(hash) {
                    debug!(
                        "Skipping pool owner reward account {}, losing {to_pay}",
                        hex::encode(hash)
                    );
                    continue;
                }

                // Check pool's reward address - removing e1 prefix
                // TODO use StakeAddress.get_hash()
                if spo.reward_account[1..] == *hash {
                    debug!(
                        "Skipping pool reward account {}, losing {to_pay}",
                        hex::encode(hash)
                    );
                    continue;
                }

                // Check if it was deregistered between staking and now
                if deregistrations.contains(hash) {
                    info!(
                        "Recently deregistered member account {}, losing {to_pay}",
                        hex::encode(hash)
                    );
                    continue;
                }

                // Transfer from reserves to this account
                rewards.push(RewardDetail {
                    account: hash.clone(),
                    rtype: RewardType::Member,
                    amount: to_pay,
                });
                total_paid += to_pay;
                delegators_paid += 1;
            }
        }

        info!(%fixed_cost, %margin_cost, leader_reward=%costs, %to_delegators, total_paid,
               delegators_paid, "Reward split:");

        costs.to_u64().unwrap_or(0)
    };

    if pay_to_pool_reward_account {
        rewards.push(RewardDetail {
            // TODO Hack to remove e1 header - needs resolving properly with StakeAddress
            account: RewardAccount::from(&spo.reward_account[1..]),
            rtype: RewardType::Leader,
            amount: spo_benefit,
        });
    } else {
        info!(
            "SPO {}'s reward account {} not paid {}",
            hex::encode(&operator_id),
            hex::encode(&spo.reward_account),
            spo_benefit,
        );
    }

    rewards
}
