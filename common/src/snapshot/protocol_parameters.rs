use crate::{protocol_params::ProtocolVersion, rational_number::RationalNumber, RewardParams};
pub use crate::{
    CostModel, CostModels, DRepVotingThresholds, ExUnitPrices, ExUnits, Lovelace,
    PoolVotingThresholds, ProtocolParamUpdate, Ratio,
};

use crate::snapshot::decode::heterogeneous_array;
use minicbor::{data::Tag, Decoder};

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

fn decode_rationale(d: &mut Decoder<'_>) -> Result<RationalNumber, minicbor::decode::Error> {
    allow_tag(d, Tag::new(30))?;
    heterogeneous_array(d, |d, assert_len| {
        assert_len(2)?;
        let numerator = d.u64()?;
        let denominator = d.u64()?;
        Ok(RationalNumber(num_rational::Ratio::new(
            numerator,
            denominator,
        )))
    })
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

#[derive(Clone)]
pub struct FutureParams(pub ProtocolParamUpdate);

pub struct CurrentParams<'a> {
    pub current: &'a ProtocolParamUpdate,
}

impl<'b, 'a> minicbor::Decode<'b, CurrentParams<'a>> for FutureParams {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        ctx: &mut CurrentParams<'a>,
    ) -> Result<Self, minicbor::decode::Error> {
        let len = d.array()?.ok_or_else(|| {
            minicbor::decode::Error::message("future_pparams must be a definite array")
        })?;

        let merged = match len {
            1 => {
                let tag = d.u8()?;
                if tag != 0 {
                    return Err(minicbor::decode::Error::message(
                        "invalid future_pparams tag for [0]",
                    ));
                }
                ctx.current.clone()
            }
            2 => {
                let tag = d.u8()?;

                match tag {
                    1 => {
                        let update: ProtocolParamUpdate = d.decode()?;
                        ctx.current.clone().merged_with(Some(update))
                    }
                    2 => match d.datatype()? {
                        minicbor::data::Type::Null => {
                            d.skip()?;
                            ctx.current.clone()
                        }
                        _ => {
                            let update: ProtocolParamUpdate = d.decode()?;
                            ctx.current.clone().merged_with(Some(update))
                        }
                    },
                    _ => {
                        return Err(minicbor::decode::Error::message(
                            "invalid future_pparams tag",
                        ))
                    }
                }
            }
            _ => {
                return Err(minicbor::decode::Error::message(
                    "invalid future_pparams shape",
                ))
            }
        };

        Ok(FutureParams(merged))
    }
}

impl ProtocolParamUpdate {
    pub fn merged_with(mut self, other: Option<ProtocolParamUpdate>) -> Self {
        let Some(o) = other else {
            return self;
        };

        if o.minfee_a.is_some() {
            self.minfee_a = o.minfee_a;
        }
        if o.minfee_b.is_some() {
            self.minfee_b = o.minfee_b;
        }
        if o.max_block_body_size.is_some() {
            self.max_block_body_size = o.max_block_body_size;
        }
        if o.max_transaction_size.is_some() {
            self.max_transaction_size = o.max_transaction_size;
        }
        if o.max_block_header_size.is_some() {
            self.max_block_header_size = o.max_block_header_size;
        }
        if o.key_deposit.is_some() {
            self.key_deposit = o.key_deposit;
        }
        if o.pool_deposit.is_some() {
            self.pool_deposit = o.pool_deposit;
        }
        if o.maximum_epoch.is_some() {
            self.maximum_epoch = o.maximum_epoch;
        }
        if o.desired_number_of_stake_pools.is_some() {
            self.desired_number_of_stake_pools = o.desired_number_of_stake_pools;
        }
        if o.pool_pledge_influence.is_some() {
            self.pool_pledge_influence = o.pool_pledge_influence;
        }
        if o.expansion_rate.is_some() {
            self.expansion_rate = o.expansion_rate;
        }
        if o.treasury_growth_rate.is_some() {
            self.treasury_growth_rate = o.treasury_growth_rate;
        }
        if o.min_pool_cost.is_some() {
            self.min_pool_cost = o.min_pool_cost;
        }
        if o.lovelace_per_utxo_word.is_some() {
            self.lovelace_per_utxo_word = o.lovelace_per_utxo_word;
        }
        if o.cost_models_for_script_languages.is_some() {
            self.cost_models_for_script_languages = o.cost_models_for_script_languages;
        }
        if o.execution_costs.is_some() {
            self.execution_costs = o.execution_costs;
        }
        if o.max_tx_ex_units.is_some() {
            self.max_tx_ex_units = o.max_tx_ex_units;
        }
        if o.max_block_ex_units.is_some() {
            self.max_block_ex_units = o.max_block_ex_units;
        }
        if o.max_value_size.is_some() {
            self.max_value_size = o.max_value_size;
        }
        if o.collateral_percentage.is_some() {
            self.collateral_percentage = o.collateral_percentage;
        }
        if o.max_collateral_inputs.is_some() {
            self.max_collateral_inputs = o.max_collateral_inputs;
        }
        if o.coins_per_utxo_byte.is_some() {
            self.coins_per_utxo_byte = o.coins_per_utxo_byte;
        }
        if o.pool_voting_thresholds.is_some() {
            self.pool_voting_thresholds = o.pool_voting_thresholds;
        }
        if o.drep_voting_thresholds.is_some() {
            self.drep_voting_thresholds = o.drep_voting_thresholds;
        }
        if o.min_committee_size.is_some() {
            self.min_committee_size = o.min_committee_size;
        }
        if o.committee_term_limit.is_some() {
            self.committee_term_limit = o.committee_term_limit;
        }
        if o.governance_action_validity_period.is_some() {
            self.governance_action_validity_period = o.governance_action_validity_period;
        }
        if o.governance_action_deposit.is_some() {
            self.governance_action_deposit = o.governance_action_deposit;
        }
        if o.drep_deposit.is_some() {
            self.drep_deposit = o.drep_deposit;
        }
        if o.drep_inactivity_period.is_some() {
            self.drep_inactivity_period = o.drep_inactivity_period;
        }
        if o.minfee_refscript_cost_per_byte.is_some() {
            self.minfee_refscript_cost_per_byte = o.minfee_refscript_cost_per_byte;
        }
        if o.decentralisation_constant.is_some() {
            self.decentralisation_constant = o.decentralisation_constant;
        }
        if o.extra_enthropy.is_some() {
            self.extra_enthropy = o.extra_enthropy;
        }
        if o.protocol_version.is_some() {
            self.protocol_version = o.protocol_version;
        }

        self
    }
}

impl<'b, C> minicbor::decode::Decode<'b, C> for ProtocolParamUpdate {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?.ok_or_else(|| {
            minicbor::decode::Error::message("ProtocolParamUpdate must be a definite array")
        })?;

        let min_fee_a = d.u32()? as u64;
        let min_fee_b = d.u32()? as u64;
        let max_block_body_size = d.u32()? as u64;
        let max_transaction_size = d.u16()? as u64;
        let max_block_header_size = d.u64()?;
        let stake_credential_deposit = d.u32()? as u64;
        let stake_pool_deposit = d.u32()? as u64;
        let stake_pool_max_retirement_epoch = d.u32()? as u64;
        let optimal_stake_pools_count = d.u16()?;
        let pledge_influence = decode_rationale(d)?;
        let monetary_expansion_rate = decode_rationale(d)?;
        let treasury_expansion_rate = decode_rationale(d)?;
        let protocol_version = decode_protocol_version(d)?;
        let min_pool_cost = d.u32()? as u64;
        let lovelace_per_utxo_byte = d.u16()? as u64;
        let cost_models = if let Some(len) = d.map()? {
            tracing::info!("cost_models map length = {}", len);

            let mut cost_models = CostModels {
                plutus_v1: None,
                plutus_v2: None,
                plutus_v3: None,
            };

            for i in 0..len {
                let lang_id: u8 = d.decode()?;
                tracing::info!("cost_model[{}] lang_id = {}", i, lang_id);

                let array_len = d.array()?;
                tracing::info!("cost_model[{}] array_len = {:?}", i, array_len);

                let mut costs = Vec::new();
                if array_len.is_none() {
                    loop {
                        match d.datatype()? {
                            minicbor::data::Type::Break => {
                                d.skip()?;
                                break;
                            }
                            _ => {
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
                    0 => cost_models.plutus_v1 = Some(cost_model),
                    1 => cost_models.plutus_v2 = Some(cost_model),
                    2 => cost_models.plutus_v3 = Some(cost_model),
                    _ => unreachable!("unexpected language version: {}", lang_id),
                }
            }

            Some(cost_models)
        } else {
            None
        };

        d.array()?;
        let mem_price = decode_rationale(d)?;
        let step_price = decode_rationale(d)?;
        let prices = ExUnitPrices {
            mem_price,
            step_price,
        };

        let max_tx_ex_units = d.decode_with(ctx)?;
        let max_block_ex_units = d.decode_with(ctx)?;
        let max_value_size = d.u16()? as u64;
        let collateral_percentage = d.u16()?;
        let max_collateral_inputs = d.u16()?;
        let pool_voting_thresholds = d.decode_with(ctx)?;
        let drep_voting_thresholds = d.decode_with(ctx)?;
        let min_committee_size = d.u16()?;
        let max_committee_term_length = d.u64()?;
        let gov_action_lifetime = d.u64()?;
        let gov_action_deposit = d.u64()?;
        let drep_deposit = d.u64()?;
        let drep_expiry = d.decode_with(ctx)?;
        let min_fee_ref_script_lovelace_per_byte = decode_rationale(d)?;

        Ok(ProtocolParamUpdate {
            minfee_a: Some(min_fee_a),
            minfee_b: Some(min_fee_b),
            max_block_body_size: Some(max_block_body_size),
            max_transaction_size: Some(max_transaction_size),
            max_block_header_size: Some(max_block_header_size),
            key_deposit: Some(stake_credential_deposit),
            pool_deposit: Some(stake_pool_deposit),
            maximum_epoch: Some(stake_pool_max_retirement_epoch),
            desired_number_of_stake_pools: Some(optimal_stake_pools_count.into()),
            pool_pledge_influence: Some(pledge_influence),
            expansion_rate: Some(monetary_expansion_rate),
            treasury_growth_rate: Some(treasury_expansion_rate),
            min_pool_cost: Some(min_pool_cost),
            lovelace_per_utxo_word: None,
            cost_models_for_script_languages: cost_models,
            execution_costs: Some(prices),
            max_tx_ex_units: Some(max_tx_ex_units),
            max_block_ex_units: Some(max_block_ex_units),
            coins_per_utxo_byte: Some(lovelace_per_utxo_byte),
            max_value_size: Some(max_value_size),
            collateral_percentage: Some(collateral_percentage.into()),
            max_collateral_inputs: Some(max_collateral_inputs.into()),
            pool_voting_thresholds: Some(pool_voting_thresholds),
            drep_voting_thresholds: Some(drep_voting_thresholds),
            min_committee_size: Some(min_committee_size.into()),
            committee_term_limit: Some(max_committee_term_length),
            governance_action_validity_period: Some(gov_action_lifetime),
            governance_action_deposit: Some(gov_action_deposit),
            drep_deposit: Some(drep_deposit),
            drep_inactivity_period: Some(drep_expiry),
            minfee_refscript_cost_per_byte: Some(min_fee_ref_script_lovelace_per_byte),
            decentralisation_constant: Some(RationalNumber::ZERO),
            extra_enthropy: None,
            protocol_version: Some(protocol_version),
        })
    }
}

impl ProtocolParamUpdate {
    pub fn to_reward_params(&self) -> Result<RewardParams, anyhow::Error> {
        Ok(RewardParams {
            expansion_rate: self
                .expansion_rate
                .clone()
                .expect("Current params must have expansion rate"),
            treasury_growth_rate: self
                .treasury_growth_rate
                .clone()
                .expect("Current params must have treasury growth rate"),
            desired_number_of_stake_pools: self
                .desired_number_of_stake_pools
                .expect("Current params must have n opt"),
            pool_pledge_influence: self
                .pool_pledge_influence
                .clone()
                .expect("Current params must have pool pledge influence"),
            min_pool_cost: self.min_pool_cost.expect("Current params must have min pool cost"),
        })
    }
}
