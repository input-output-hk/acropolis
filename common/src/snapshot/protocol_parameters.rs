use crate::rational_number::RationalNumber;
use crate::{protocol_params::ProtocolVersion, snapshot::streaming_snapshot::Epoch};
pub use crate::{
    CostModel, CostModels, DRepVotingThresholds, ExUnitPrices, ExUnits, Lovelace,
    PoolVotingThresholds, ProtocolParamUpdate, Ratio,
};

use crate::snapshot::decode::heterogeneous_array;
use minicbor::{data::Tag, Decoder};

/// Model from https://github.com/IntersectMBO/formal-ledger-specifications/blob/master/src/Ledger/PParams.lagda
/// Some of the names have been adapted to improve readability.
/// Also see https://github.com/IntersectMBO/cardano-ledger/blob/d90eb4df4651970972d860e95f1a3697a3de8977/eras/conway/impl/cddl-files/conway.cddl#L324
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolParameters {
    // Outside of all groups.
    pub protocol_version: ProtocolVersion,

    // Network group
    pub max_block_body_size: u64,
    pub max_transaction_size: u64,
    pub max_block_header_size: u16,
    pub max_tx_ex_units: ExUnits,
    pub max_block_ex_units: ExUnits,
    pub max_value_size: u64,
    pub max_collateral_inputs: u16,

    // Economic group
    pub min_fee_a: Lovelace,
    pub min_fee_b: u64,
    pub stake_credential_deposit: Lovelace,
    pub stake_pool_deposit: Lovelace,
    pub monetary_expansion_rate: Ratio,
    pub treasury_expansion_rate: Ratio,
    pub min_pool_cost: u64,
    pub lovelace_per_utxo_byte: Lovelace,
    pub prices: ExUnitPrices,
    pub min_fee_ref_script_lovelace_per_byte: Ratio,
    pub max_ref_script_size_per_tx: u32,
    pub max_ref_script_size_per_block: u32,
    pub ref_script_cost_stride: u32,
    pub ref_script_cost_multiplier: Ratio,

    // Technical group
    pub stake_pool_max_retirement_epoch: Epoch,
    pub optimal_stake_pools_count: u16,
    pub pledge_influence: Ratio,
    pub collateral_percentage: u16,
    pub cost_models: CostModels,

    // Governance group
    pub pool_voting_thresholds: PoolVotingThresholds,
    pub drep_voting_thresholds: DRepVotingThresholds,
    pub min_committee_size: u16,
    pub max_committee_term_length: Epoch,
    pub gov_action_lifetime: Epoch,
    pub gov_action_deposit: Lovelace,
    pub drep_deposit: Lovelace,
    pub drep_expiry: Epoch,
}

fn allow_tag(d: &mut Decoder<'_>, expected: Tag) -> Result<(), minicbor::decode::Error> {
    if d.datatype()? == minicbor::data::Type::Tag {
        let tag = d.tag()?;
        if tag != expected {
            return Err(minicbor::decode::Error::message(format!(
                "invalid CBOR tag: expected {expected} got {tag}"
            )));
        }
    }

    Ok(())
}

fn decode_rationale(d: &mut Decoder<'_>) -> Result<Ratio, minicbor::decode::Error> {
    allow_tag(d, Tag::new(30))?;
    heterogeneous_array(d, |d, assert_len| {
        assert_len(2)?;
        let numerator = d.u64()?;
        let denominator = d.u64()?;
        Ok(Ratio {
            numerator,
            denominator,
        })
    })
}

impl Default for ProtocolParameters {
    fn default() -> Self {
        ProtocolParameters {
            protocol_version: ProtocolVersion { major: 0, minor: 0 },
            min_fee_a: 0,
            min_fee_b: 0,
            max_block_body_size: 0,
            max_transaction_size: 0,
            max_block_header_size: 0,
            stake_credential_deposit: 0,
            stake_pool_deposit: 0,
            stake_pool_max_retirement_epoch: 0,
            optimal_stake_pools_count: 0,
            pledge_influence: Ratio {
                numerator: 0,
                denominator: 1,
            },
            monetary_expansion_rate: Ratio {
                numerator: 0,
                denominator: 1,
            },
            treasury_expansion_rate: Ratio {
                numerator: 0,
                denominator: 1,
            },
            min_pool_cost: 0,
            lovelace_per_utxo_byte: 0,
            cost_models: CostModels {
                plutus_v1: None,
                plutus_v2: None,
                plutus_v3: None,
            },
            prices: ExUnitPrices {
                mem_price: RationalNumber::from(0, 1),
                step_price: RationalNumber::from(0, 1),
            },
            max_tx_ex_units: ExUnits { mem: 0, steps: 0 },
            max_block_ex_units: ExUnits { mem: 0, steps: 0 },
            max_value_size: 0,
            collateral_percentage: 0,
            max_collateral_inputs: 0,
            pool_voting_thresholds: PoolVotingThresholds {
                motion_no_confidence: RationalNumber::from(0, 1),
                committee_normal: RationalNumber::from(0, 1),
                committee_no_confidence: RationalNumber::from(0, 1),
                hard_fork_initiation: RationalNumber::from(0, 1),
                security_voting_threshold: RationalNumber::from(0, 1),
            },
            drep_voting_thresholds: DRepVotingThresholds {
                motion_no_confidence: RationalNumber::from(0, 1),
                committee_normal: RationalNumber::from(0, 1),
                committee_no_confidence: RationalNumber::from(0, 1),
                update_constitution: RationalNumber::from(0, 1),
                hard_fork_initiation: RationalNumber::from(0, 1),
                pp_network_group: RationalNumber::from(0, 1),
                pp_economic_group: RationalNumber::from(0, 1),
                pp_technical_group: RationalNumber::from(0, 1),
                pp_governance_group: RationalNumber::from(0, 1),
                treasury_withdrawal: RationalNumber::from(0, 1),
            },
            min_committee_size: 0,
            max_committee_term_length: 0,
            gov_action_lifetime: 0,
            gov_action_deposit: 0,
            drep_deposit: 0,
            drep_expiry: 0,
            min_fee_ref_script_lovelace_per_byte: Ratio {
                numerator: 0,
                denominator: 1,
            },
            max_ref_script_size_per_tx: 0,
            max_ref_script_size_per_block: 0,
            ref_script_cost_stride: 0,
            ref_script_cost_multiplier: Ratio {
                numerator: 1,
                denominator: 1,
            },
        }
    }
}

fn decode_protocol_version(
    d: &mut Decoder<'_>,
) -> Result<ProtocolVersion, minicbor::decode::Error> {
    heterogeneous_array(d, |d, assert_len| {
        assert_len(2)?;
        let major: u8 = d.u8()?;

        // See: https://github.com/IntersectMBO/cardano-ledger/blob/693218df6cd90263da24e6c2118bac420ceea3a1/eras/conway/impl/cddl-files/conway.cddl#L126
        if major > 12 {
            return Err(minicbor::decode::Error::message(
                "invalid protocol version's major: too high",
            ));
        }
        Ok(ProtocolVersion {
            major: major as u64,
            minor: d.u64()?,
        })
    })
}

impl<'b, C> minicbor::decode::Decode<'b, C> for ProtocolParameters {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;

        // Check first field - for future params it might be a variant tag
        let first_field_type = d.datatype()?;

        // Peek at the value if it's U8
        if first_field_type == minicbor::data::Type::U8 {
            let tag_value = d.u8()?;

            // future_pparams = [0] / [1, pparams_real] / [2, strict_maybe<pparams_real>]
            if tag_value == 0 {
                // Return default/empty protocol parameters
                return Ok(ProtocolParameters::default());
            } else if tag_value == 1 {
                // Continue with normal parsing below (tag already consumed)
            } else if tag_value == 2 {
                // Next element might be Nothing or Just(params)
                // For now, skip this case
                return Err(minicbor::decode::Error::message(
                    "Future params variant [2] not yet implemented",
                ));
            } else {
                // Not a variant tag, this is the actual first field (max_block_header_size?)
                let unknown_field = tag_value;
                return Self::decode_real_params(d, ctx, unknown_field);
            }
        }

        // If we get here, we have variant [1] or [2] and need to parse the real params
        // For variant [1], the next field should be the start of pparams_real
        // which starts with the first real field (not a tag)
        let first_field_type = d.datatype()?;
        if first_field_type == minicbor::data::Type::U8 {
            let first_field = d.u8()?;
            Self::decode_real_params(d, ctx, first_field)
        } else {
            Err(minicbor::decode::Error::message(
                "Expected U8 for first field of pparams_real",
            ))
        }
    }
}

impl ProtocolParameters {
    fn decode_real_params<'b, C>(
        d: &mut minicbor::Decoder<'b>,
        ctx: &mut C,
        first_field: u8,
    ) -> Result<Self, minicbor::decode::Error> {
        // first_field is field 0 which we already consumed (U8=44 or similar, unknown purpose)

        // Read what appears to be the fee parameters
        let min_fee_a = d.u32()? as u64;
        let min_fee_b = d.u32()? as u64;

        // Read what appears to be size limits (but check types - they might be u16 not u64)
        let max_block_body_size = d.u16()? as u64;
        let max_transaction_size = d.u16()? as u64;

        // Deposits
        let stake_credential_deposit = d.u32()? as u64;
        let stake_pool_deposit = d.u32()? as u64;

        // Retirement epoch
        let stake_pool_max_retirement_epoch = d.u8()? as u64;

        // Pool count
        let optimal_stake_pools_count = d.u16()?;

        // Fields 9-11 should be ratios (Tag 30)
        let pledge_influence = decode_rationale(d)?;
        let monetary_expansion_rate = decode_rationale(d)?;
        let treasury_expansion_rate = decode_rationale(d)?;

        // Field 12 should be protocol version array
        let protocol_version = decode_protocol_version(d)?;

        // Field 13
        let min_pool_cost = d.u32()? as u64;

        // Field 14
        let lovelace_per_utxo_byte = d.u16()? as u64;

        // Field 15: cost_models map - manually decode since CostModel format might be different
        let mut plutus_v1 = None;
        let mut plutus_v2 = None;
        let mut plutus_v3 = None;

        let map_len = d.map()?;

        if let Some(len) = map_len {
            for _ in 0..len {
                let lang_id: u8 = d.decode()?;

                // Try decoding as array of i64 (could be indefinite)
                let array_len = d.array()?;

                let mut costs = Vec::new();
                if array_len.is_none() {
                    // Indefinite array - read until break
                    loop {
                        match d.datatype()? {
                            minicbor::data::Type::Break => {
                                d.skip()?; // consume the break
                                break;
                            }
                            _ => {
                                // Decode as i64, handling different integer sizes
                                let cost: i64 = d.decode()?;
                                costs.push(cost);
                            }
                        }
                    }
                } else if let Some(alen) = array_len {
                    for _ in 0..alen {
                        let cost: i64 = d.decode()?;
                        costs.push(cost);
                    }
                }

                let cost_model = CostModel::new(costs);
                match lang_id {
                    0 => plutus_v1 = Some(cost_model),
                    1 => plutus_v2 = Some(cost_model),
                    2 => plutus_v3 = Some(cost_model),
                    _ => unreachable!("unexpected language version: {}", lang_id),
                }
            }
        }

        // Field 16: prices - encoded as array containing two tag-30 ratios
        d.array()?; // Outer array
        let mem_price = decode_rationale(d)?; // First ratio (tag 30)
        let step_price = decode_rationale(d)?; // Second ratio (tag 30)
        let prices = ExUnitPrices {
            mem_price: RationalNumber::from(mem_price.numerator, mem_price.denominator),
            step_price: RationalNumber::from(step_price.numerator, step_price.denominator),
        };

        // Field 17: max_tx_ex_units
        let max_tx_ex_units = d.decode_with(ctx)?;

        // Field 18: max_block_ex_units
        let max_block_ex_units = d.decode_with(ctx)?;

        // Field 19: max_value_size
        let max_value_size = d.u16()? as u64;

        // Field 20: collateral_percentage
        let collateral_percentage = d.u16()?;

        // Field 21: max_collateral_inputs
        let max_collateral_inputs = d.u16()?;

        // Field 22: pool_voting_thresholds
        let pool_voting_thresholds = d.decode_with(ctx)?;

        // Field 23: drep_voting_thresholds
        let drep_voting_thresholds = d.decode_with(ctx)?;

        // Field 24: min_committee_size
        let min_committee_size = d.u16()?;

        // Field 25: max_committee_term_length
        let max_committee_term_length = d.u64()?;

        // Field 26: gov_action_lifetime
        let gov_action_lifetime = d.u64()?;

        // Field 27: gov_action_deposit
        let gov_action_deposit = d.u64()?;

        // Field 28: drep_deposit
        let drep_deposit = d.u64()?;

        // Field 29: drep_expiry
        let drep_expiry = d.decode_with(ctx)?;

        // Field 30: min_fee_ref_script_lovelace_per_byte
        let min_fee_ref_script_lovelace_per_byte = decode_rationale(d)?;

        // Field 0 (U8=44) - still unknown, need to determine max_block_header_size
        let max_block_header_size = first_field as u16;

        Ok(ProtocolParameters {
            protocol_version,
            min_fee_a,
            min_fee_b,
            max_block_body_size,
            max_transaction_size,
            max_block_header_size,
            stake_credential_deposit,
            stake_pool_deposit,
            stake_pool_max_retirement_epoch,
            optimal_stake_pools_count,
            pledge_influence,
            monetary_expansion_rate,
            treasury_expansion_rate,
            min_pool_cost,
            lovelace_per_utxo_byte,
            cost_models: CostModels {
                plutus_v1,
                plutus_v2,
                plutus_v3,
            },
            prices,
            max_tx_ex_units,
            max_block_ex_units,
            max_value_size,
            collateral_percentage,
            max_collateral_inputs,
            pool_voting_thresholds,
            drep_voting_thresholds,
            min_committee_size,
            max_committee_term_length,
            gov_action_lifetime,
            gov_action_deposit,
            drep_deposit,
            drep_expiry,
            min_fee_ref_script_lovelace_per_byte,
            max_ref_script_size_per_tx: 200 * 1024,
            max_ref_script_size_per_block: 1024 * 1024,
            ref_script_cost_stride: 25600,
            ref_script_cost_multiplier: Ratio {
                numerator: 12,
                denominator: 10,
            },
        })
    }
}
