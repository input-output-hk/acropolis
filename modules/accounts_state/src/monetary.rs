//! Acropolis AccountsState: monetary (reserves, treasury) calculations

use acropolis_common::{
    Lovelace, ShelleyParams,
    rational_number::RationalNumber,
};
use crate::state::Pots;
use anyhow::{Result, anyhow};
use tracing::info;
use bigdecimal::{BigDecimal, ToPrimitive, One};
use std::str::FromStr;

/// Result of monetary calculation
#[derive(Debug, Default, Clone)]
pub struct MonetaryResult {

    /// Updated pots
    pub pots: Pots,

    /// Total stake reward available
    pub stake_rewards: BigDecimal,
}

/// Calculate monetary change at the start of an epoch, returning updated pots and total
/// available for stake rewards
pub fn calculate_monetary_change(params: &ShelleyParams,
                                 old_pots: &Pots, total_fees: Lovelace,
                                 total_non_obft_blocks: usize) -> Result<MonetaryResult> {
    let mut new_pots = old_pots.clone();

    // Handle monetary expansion - movement from reserves to rewards and treasury
    let eta = calculate_eta(params, total_non_obft_blocks)?;
    let monetary_expansion = calculate_monetary_expansion(&params, old_pots.reserves, &eta);

    // Total rewards available is monetary expansion plus fees from previous epoch
    let total_reward_pot = &monetary_expansion + BigDecimal::from(total_fees);

    // Top-slice some for treasury
    let treasury_cut = RationalNumber::new(2, 10);
    // TODO odd values again! &params.protocol_params.treasury_cut;  // Tau
    let treasury_increase = (&total_reward_pot
                             * BigDecimal::from(treasury_cut.numer())
                             / BigDecimal::from(treasury_cut.denom()))
        .with_scale(0);

    let treasury_increase_u64 = treasury_increase
        .to_u64()
        .ok_or(anyhow!("Can't calculate integral treasury cut"))?;

    info!(total_rewards=%total_reward_pot, cut=%treasury_cut, increase=treasury_increase_u64,
          "Treasury:");

    new_pots.treasury += treasury_increase_u64;
    new_pots.reserves -= treasury_increase_u64;

    Ok(MonetaryResult {
        pots: new_pots,
        stake_rewards: total_reward_pot - treasury_increase,
    })
}

// Calculate 'eta' - ratio of blocks produced during the epoch vs expected
fn calculate_eta(params: &ShelleyParams, total_non_obft_blocks: usize) -> Result<BigDecimal> {
    let decentralisation = &params.protocol_params.decentralisation_param;
    let active_slots_coeff = BigDecimal::from_str(&params.active_slots_coeff.to_string())?;
    let epoch_length = BigDecimal::from(params.epoch_length);

    let eta = if decentralisation >= &RationalNumber::new(8,10) {
        BigDecimal::one()
    } else {
        let expected_blocks = epoch_length * active_slots_coeff *
            (BigDecimal::one() - BigDecimal::from(decentralisation.numer())
             / BigDecimal::from(decentralisation.denom()));

        (BigDecimal::from(total_non_obft_blocks as u64) / expected_blocks)
            .min(BigDecimal::one())
    };

    Ok(eta)
}

// Calculate monetary expansion based on current reserves
fn calculate_monetary_expansion(params: &ShelleyParams, reserves: Lovelace, eta: &BigDecimal)
                                -> BigDecimal {
    let monetary_expansion_factor = RationalNumber::new(3, 1000);
    // TODO odd values coming in! &params.protocol_params.monetary_expansion; // Rho
    let monetary_expansion = (BigDecimal::from(reserves)
                              * eta
                              * BigDecimal::from(monetary_expansion_factor.numer())
                              / BigDecimal::from(monetary_expansion_factor.denom()))
        .with_scale(0);

    info!(eta=%eta, rho=%monetary_expansion_factor, %monetary_expansion, "Monetary:");

    monetary_expansion
}
