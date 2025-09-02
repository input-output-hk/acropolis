//! Acropolis AccountsState: rewards calculations

use crate::snapshot::{Snapshot, SnapshotSPO};
use acropolis_common::{
    protocol_params::ShelleyParams, rational_number::RationalNumber, KeyHash, Lovelace,
    RewardAccount, SPORewards,
};
use anyhow::{bail, Result};
use bigdecimal::{BigDecimal, One, ToPrimitive, Zero};
use std::cmp::min;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Result of a rewards calculation
#[derive(Debug, Default)]
pub struct RewardsResult {
    /// Total rewards paid
    pub total_paid: u64,

    /// Rewards to be paid
    pub rewards: Vec<(RewardAccount, Lovelace)>,

    /// SPO rewards
    pub spo_rewards: Vec<(KeyHash, SPORewards)>,
}

/// Calculate rewards for a given epoch based on current rewards state and protocol parameters
/// The epoch is the one we are now entering - we assume the snapshot for this has already been
/// taken.
/// Note immutable - only state change allowed is to push a new snapshot
pub fn calculate_rewards(
    epoch: u64,
    performance: Arc<Snapshot>,
    staking: Arc<Snapshot>,
    params: &ShelleyParams,
    stake_rewards: Lovelace,
) -> Result<RewardsResult> {
    let mut result = RewardsResult::default();

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
    info!(epoch, %total_supply, %stake_rewards, total_blocks, "Calculating rewards:");

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
        let blocks_produced =
            performance.spos.get(operator_id).map(|s| s.blocks_produced).unwrap_or(0);

        if blocks_produced == 0 {
            continue;
        }

        calculate_spo_rewards(
            operator_id,
            spo,
            blocks_produced as u64,
            total_blocks,
            &stake_rewards,
            &total_supply,
            &relative_pool_saturation_size,
            &pledge_influence_factor,
            params,
            staking.clone(),
            &mut result,
            &mut total_paid_to_pools,
            &mut total_paid_to_delegators,
            &mut num_pools_paid,
            &mut num_delegators_paid,
        );
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
    relative_pool_saturation_size: &BigDecimal,
    pledge_influence_factor: &BigDecimal,
    params: &ShelleyParams,
    staking: Arc<Snapshot>,
    result: &mut RewardsResult,
    total_paid_to_pools: &mut Lovelace,
    total_paid_to_delegators: &mut Lovelace,
    num_pools_paid: &mut usize,
    num_delegators_paid: &mut usize,
) {
    // Actual blocks produced as proportion of epoch (Beta)
    let relative_blocks = BigDecimal::from(blocks_produced) / BigDecimal::from(total_blocks as u64);

    // Active stake (sigma)
    let pool_stake = BigDecimal::from(spo.total_stake);
    if pool_stake.is_zero() {
        // No stake, no rewards or earnings
        return;
    }

    // Get the stake actually delegated by the owners accounts to this SPO
    let pool_owner_stake =
        staking.get_stake_delegated_to_spo_by_addresses(&operator_id, &spo.pool_owners);

    // If they haven't met their pledge, no dice
    if pool_owner_stake < spo.pledge {
        warn!(
            "SPO {} has owner stake {} less than pledge {} - skipping",
            hex::encode(&operator_id),
            pool_owner_stake,
            spo.pledge
        );
        return;
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
        relative_blocks.clone() / relative_pool_stake.clone()
    };

    // Get actual pool rewards
    let pool_rewards = (&optimum_rewards * &pool_performance).with_scale(0);

    info!(blocks=blocks_produced, %pool_stake, %relative_pool_stake, %relative_blocks,
          %pool_performance, %optimum_rewards, %pool_rewards,
           "Pool {}", hex::encode(operator_id.clone()));

    // Subtract fixed costs
    let fixed_cost = BigDecimal::from(spo.fixed_cost);
    let spo_benefit = if pool_rewards <= fixed_cost {
        info!("Rewards < cost - all paid to SPO");

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

        // You'd think this was just pool_rewards-costs here, but the Haskell code recalculates
        // the margin without the relative_owner_stake term !?
        let to_delegators =
            ((&pool_rewards - &fixed_cost) * (BigDecimal::one() - &margin)).with_scale(0);
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
                    info!(
                        "Skipping pool owner reward account {}, losing {to_pay}",
                        hex::encode(hash)
                    );
                    continue;
                }

                // TODO Shelley-until-Allegra bug if same reward account used for multiple
                // SPOs - check pool's reward address but only before Allegra?

                // Transfer from reserves to this account
                result.rewards.push((hash.clone(), to_pay));
                result.total_paid += to_pay;

                total_paid += to_pay;
                delegators_paid += 1;
            }
        }

        info!(%fixed_cost, %margin_cost, leader_reward=%costs, %to_delegators, total_paid,
              delegators_paid, "Reward split:");

        *num_delegators_paid += delegators_paid;
        *total_paid_to_delegators += total_paid;
        costs.to_u64().unwrap_or(0)
    };
    result.rewards.push((spo.reward_account.clone(), spo_benefit));
    result.spo_rewards.push((
        operator_id.clone(),
        SPORewards {
            total_rewards: pool_rewards.to_u64().unwrap_or(0),
            operator_rewards: spo_benefit,
        },
    ));
    result.total_paid += spo_benefit;
    *total_paid_to_pools += spo_benefit;
    *num_pools_paid += 1;
}
