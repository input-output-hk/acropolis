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
    pub spors: Vec<(KeyHash, SPORewards)>,
}

/// State for rewards calculation
#[derive(Debug, Default, Clone)]
pub struct RewardsState {
    /// Latest snapshot (epoch i) (if any)
    pub mark: Arc<Snapshot>,

    /// Previous snapshot (epoch i-1) (if any)
    pub set: Arc<Snapshot>,

    /// One before that (epoch i-2) (if any)
    pub go: Arc<Snapshot>,
}

impl RewardsState {
    /// Push a new snapshot
    pub fn push(&mut self, latest: Snapshot) {
        self.go = self.set.clone();
        self.set = self.mark.clone();
        self.mark = Arc::new(latest);
    }

    /// Calculate rewards for a given epoch based on current rewards state and protocol parameters
    /// The epoch is the one we are now entering - we assume the snapshot for this has already been
    /// taken.
    /// Note immutable - only state change allowed is to push a new snapshot
    pub fn calculate_rewards(
        &self,
        epoch: u64,
        params: &ShelleyParams,
        total_blocks: usize,
        stake_rewards: BigDecimal,
    ) -> Result<RewardsResult> {
        let mut result = RewardsResult::default();

        // Calculate total supply (total in circulation + treasury) or
        // equivalently (max-supply-reserves) - this is the denominator
        // for sigma, z0, s
        let total_supply = BigDecimal::from(params.max_lovelace_supply - self.mark.pots.reserves);
        info!(epoch, %total_supply, %stake_rewards, "Calculating rewards:");

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
        let mut num_delegators_paid: usize = 0;
        for (operator_id, spo) in self.go.spos.iter() {
            // Actual blocks produced for epoch (i)
            let blocks_produced = {
                if let Some(s) = self.mark.spos.get(operator_id) {
                    s.blocks_produced
                } else {
                    0
                }
            };

            Self::calculate_spo_rewards(
                operator_id,
                spo,
                blocks_produced as u64,
                total_blocks as u64,
                &stake_rewards,
                &total_supply,
                &relative_pool_saturation_size,
                &pledge_influence_factor,
                params,
                self.go.clone(),
                &mut result,
                &mut total_paid_to_pools,
                &mut total_paid_to_delegators,
                &mut num_delegators_paid,
            );
        }

        info!(
            num_delegators_paid,
            total_paid_to_delegators,
            total_paid_to_pools,
            total = result.total_paid,
            "Rewards actually paid:"
        );

        Ok(result)
    }

    fn calculate_spo_rewards(
        operator_id: &KeyHash,
        spo: &SnapshotSPO,
        blocks_produced: u64,
        total_blocks: u64,
        stake_rewards: &BigDecimal,
        total_supply: &BigDecimal,
        relative_pool_saturation_size: &BigDecimal,
        pledge_influence_factor: &BigDecimal,
        params: &ShelleyParams,
        snapshot: Arc<Snapshot>,
        result: &mut RewardsResult,
        total_paid_to_pools: &mut Lovelace,
        total_paid_to_delegators: &mut Lovelace,
        num_delegators_paid: &mut usize,
    ) {
        // Actual blocks produced as proportion of epoch (Beta)
        let relative_blocks = BigDecimal::from(blocks_produced) / BigDecimal::from(total_blocks);

        // Active stake (sigma)
        let pool_stake = BigDecimal::from(spo.total_stake);
        if pool_stake.is_zero() {
            // No stake, no rewards or earnings
            return;
        }

        // Get the stake actually delegated by the owners accounts to this SPO
        let pool_owner_stake =
            snapshot.get_stake_delegated_to_spo_by_addresses(&operator_id, &spo.pool_owners);

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
        let capped_relative_pool_pledge =
            min(&relative_pool_pledge, &relative_pool_saturation_size);

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
            let margin = ((&pool_rewards - &fixed_cost)
                          * BigDecimal::from(spo.margin.numerator)  // TODO use RationalNumber
                          / BigDecimal::from(spo.margin.denominator))
            .with_scale(0);
            let costs = &fixed_cost + &margin;
            let remainder = &pool_rewards - &costs;

            // Pay the delegators - split remainder in proportional to delegated stake,
            // * as it was 2 epochs ago *
            let to_delegators = remainder.to_u64().unwrap_or(0);
            if to_delegators > 0 {
                let total_stake = BigDecimal::from(spo.total_stake);
                for (hash, stake) in &spo.delegators {
                    let proportion = BigDecimal::from(stake) / &total_stake;

                    // and hence how much of the total reward they get
                    let reward = BigDecimal::from(to_delegators) * &proportion;
                    let to_pay = reward.with_scale(0).to_u64().unwrap_or(0);

                    debug!("Reward stake {stake} -> proportion {proportion} of SPO rewards {to_delegators} -> {to_pay} to hash {}",
                           hex::encode(&hash));

                    // Transfer from reserves to this account
                    result.rewards.push((hash.clone(), to_pay));
                    result.total_paid += to_pay;

                    *num_delegators_paid += 1;
                    *total_paid_to_delegators += to_pay;
                }
            }

            info!(%fixed_cost, %margin, to_delegators, "Reward split:");

            costs.to_u64().unwrap_or(0)
        };
        result.rewards.push((spo.reward_account.clone(), spo_benefit));
        result.spors.push((
            operator_id.clone(),
            SPORewards {
                total_rewards: pool_rewards.to_u64().unwrap_or(0),
                operator_rewards: spo_benefit,
            },
        ));
        result.total_paid += spo_benefit;
        *total_paid_to_pools += spo_benefit;
    }
}
