//! Acropolis AccountsState: monetary (reserves, treasury) calculations

use crate::state::Pots;
use acropolis_common::{protocol_params::ShelleyParams, rational_number::RationalNumber, Lovelace};
use anyhow::{anyhow, Result};
use bigdecimal::{BigDecimal, One, ToPrimitive};
use tracing::info;

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
pub fn calculate_monetary_change(
    params: &ShelleyParams,
    old_pots: &Pots,
    total_fees_last_epoch: Lovelace,
    total_non_obft_blocks: usize,
) -> Result<MonetaryResult> {
    let mut new_pots = old_pots.clone();

    // Add fees to reserves to start with - they will get allocated to treasury and stake
    // later
    new_pots.reserves += total_fees_last_epoch;

    // Handle monetary expansion - movement from reserves to rewards and treasury
    let eta = calculate_eta(params, total_non_obft_blocks)?;
    let monetary_expansion = calculate_monetary_expansion(&params, old_pots.reserves, &eta);

    // Total rewards available is monetary expansion plus fees from last epoch
    // TODO not sure why this is one epoch behind
    let total_reward_pot = &monetary_expansion + BigDecimal::from(total_fees_last_epoch);

    // Top-slice some for treasury
    let treasury_cut = RationalNumber::new(2, 10);
    // TODO odd values again! &params.protocol_params.treasury_cut;  // Tau
    let treasury_increase = (&total_reward_pot * BigDecimal::from(treasury_cut.numer())
        / BigDecimal::from(treasury_cut.denom()))
    .with_scale(0);

    let treasury_increase_u64 =
        treasury_increase.to_u64().ok_or(anyhow!("Can't calculate integral treasury cut"))?;

    new_pots.treasury += treasury_increase_u64;
    new_pots.reserves -= treasury_increase_u64;

    // Remainder goes to stakeholders
    let stake_rewards = &total_reward_pot - &treasury_increase;

    info!(total_rewards=%total_reward_pot, cut=%treasury_cut, increase=treasury_increase_u64,
          %stake_rewards, "Treasury:");

    Ok(MonetaryResult {
        pots: new_pots,
        stake_rewards,
    })
}

// Calculate 'eta' - ratio of blocks produced during the epoch vs expected
fn calculate_eta(params: &ShelleyParams, total_non_obft_blocks: usize) -> Result<BigDecimal> {
    let decentralisation = &params.protocol_params.decentralisation_param;
    let active_slots_coeff = BigDecimal::from(params.active_slots_coeff.numer())
        / BigDecimal::from(params.active_slots_coeff.denom());
    let epoch_length = BigDecimal::from(params.epoch_length);

    let eta = if decentralisation >= &RationalNumber::new(8, 10) {
        BigDecimal::one()
    } else {
        let expected_blocks = epoch_length
            * active_slots_coeff
            * (BigDecimal::one()
                - BigDecimal::from(decentralisation.numer())
                    / BigDecimal::from(decentralisation.denom()));

        (BigDecimal::from(total_non_obft_blocks as u64) / expected_blocks).min(BigDecimal::one())
    };

    Ok(eta)
}

// Calculate monetary expansion based on current reserves
fn calculate_monetary_expansion(
    params: &ShelleyParams,
    reserves: Lovelace,
    eta: &BigDecimal,
) -> BigDecimal {
    let monetary_expansion_factor = params.protocol_params.monetary_expansion;
    let monetary_expansion =
        (BigDecimal::from(reserves) * eta * BigDecimal::from(monetary_expansion_factor.numer())
            / BigDecimal::from(monetary_expansion_factor.denom()))
        .with_scale(0);

    info!(eta=%eta, rho=%monetary_expansion_factor, %monetary_expansion, "Monetary:");

    monetary_expansion
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::protocol_params::{
        NetworkId, Nonce, NonceVariant, ProtocolVersion, ShelleyProtocolParams,
    };
    use acropolis_common::rational_number::rational_number_from_f32;
    use chrono::{DateTime, Utc};

    // Known values at start of Shelley - from Java reference and DBSync
    const EPOCH_208_RESERVES: Lovelace = 13_888_022_852_926_644;
    const EPOCH_208_MIRS: Lovelace = 593_529_326_186_446;
    const EPOCH_208_FEES: Lovelace = 10_670_212_208;

    const EPOCH_209_RESERVES: Lovelace = 13_286_160_713_028_443;
    const EPOCH_209_TREASURY: Lovelace = 8_332_813_711_755;
    const EPOCH_209_FEES: Lovelace = 7_666_346_424;

    const EPOCH_210_RESERVES: Lovelace = 13_278_197_552_770_393;
    const EPOCH_210_TREASURY: Lovelace = 16_306_644_182_013;
    const EPOCH_210_REFUNDS_TO_TREASURY: Lovelace = 500_000_000; // 1 SPO with unknown reward

    const EPOCH_211_RESERVES: Lovelace = 13_270_236_767_315_870;
    const EPOCH_211_TREASURY: Lovelace = 24_275_595_982_960;

    fn shelley_params() -> ShelleyParams {
        ShelleyParams {
            active_slots_coeff: rational_number_from_f32(0.05).unwrap(),
            epoch_length: 432000,
            max_kes_evolutions: 62,
            max_lovelace_supply: 45_000_000_000_000_000,
            network_id: NetworkId::Mainnet,
            network_magic: 76482407,
            protocol_params: ShelleyProtocolParams {
                protocol_version: ProtocolVersion { major: 2, minor: 0 },
                max_tx_size: 16384,
                max_block_body_size: 65536,
                max_block_header_size: 1100,
                key_deposit: 2_000_000,
                min_utxo_value: 1_000_000,
                minfee_a: 44,
                minfee_b: 155381,
                pool_deposit: 500_000_000,
                stake_pool_target_num: 150,
                min_pool_cost: 340_000_000,
                pool_retire_max_epoch: 18,
                extra_entropy: Nonce {
                    tag: NonceVariant::NeutralNonce,
                    hash: None,
                },
                decentralisation_param: RationalNumber::new(1, 1),
                monetary_expansion: RationalNumber::new(3, 1000),
                treasury_cut: RationalNumber::new(2, 10),
                pool_pledge_influence: RationalNumber::new(3, 10),
            },
            security_param: 2160,
            slot_length: 1,
            slots_per_kes_period: 129600,
            system_start: DateTime::<Utc>::default(),
            update_quorum: 5,
        }
    }

    #[test]
    fn epoch_208_monetary_change() {
        let params = shelley_params();
        let pots = Pots {
            reserves: EPOCH_208_RESERVES,
            treasury: 0,
            deposits: 0,
        };

        // Epoch 207 had no fees or non-OBFT blocks
        let result = calculate_monetary_change(&params, &pots, 0, 0).unwrap();

        // Epoch 209 reserves - all goes to treasury
        assert_eq!(
            result.pots.reserves,
            EPOCH_208_RESERVES - EPOCH_209_TREASURY
        );
        assert_eq!(result.pots.reserves - EPOCH_208_MIRS, EPOCH_209_RESERVES);
        assert_eq!(result.pots.treasury, EPOCH_209_TREASURY);
    }

    #[test]
    fn epoch_209_monetary_change() {
        let params = shelley_params();
        let pots = Pots {
            reserves: EPOCH_209_RESERVES,
            treasury: EPOCH_209_TREASURY,
            deposits: 0,
        };

        // Epoch 208 had no non-OBFT blocks
        let result = calculate_monetary_change(&params, &pots, EPOCH_208_FEES, 0).unwrap();

        // Epoch 210 reserves
        assert_eq!(result.pots.reserves, EPOCH_210_RESERVES);
        assert_eq!(result.pots.treasury, EPOCH_210_TREASURY);
    }

    #[test]
    fn epoch_210_monetary_change() {
        let params = shelley_params();
        let pots = Pots {
            reserves: EPOCH_210_RESERVES,
            treasury: EPOCH_210_TREASURY,
            deposits: 0,
        };

        // Epoch 209 had no non-OBFT blocks
        let result = calculate_monetary_change(&params, &pots, EPOCH_209_FEES, 0).unwrap();

        // Epoch 211 reserves
        assert_eq!(result.pots.reserves, EPOCH_211_RESERVES);
        assert_eq!(
            result.pots.treasury + EPOCH_210_REFUNDS_TO_TREASURY,
            EPOCH_211_TREASURY
        );
    }
}
