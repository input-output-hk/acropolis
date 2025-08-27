use acropolis_common::{
    protocol_params::{Nonce, NonceVariant, ProtocolParams},
    rest_helper::ToCheckedF64,
    VotingProcedure,
};
use num_traits::ToPrimitive;
use rust_decimal::Decimal;
use serde::Serialize;

/// REST response structure for proposal votes
#[derive(Serialize)]
pub struct VoteRest {
    pub transaction: String,
    pub voting_procedure: VotingProcedure,
}

#[derive(Serialize)]
pub struct PoolExtendedRest {
    pub pool_id: String,
    pub hex: String,
    pub active_stake: String, // u64 in string
    pub live_stake: String,   // u64 in string
    pub blocks_minted: u64,
    pub live_saturation: Decimal,
    pub declared_pledge: String, // u64 in string
    pub margin_cost: f32,
    pub fixed_cost: String, // u64 in string
}

// REST response structure for protocol params
#[derive(Serialize)]
pub struct ProtocolParamsRest {
    pub epoch: u64,
    pub min_fee_a: u32,
    pub min_fee_b: u32,
    pub max_block_size: u32,
    pub max_tx_size: u32,
    pub max_block_header_size: u32,
    pub key_deposit: String,
    pub pool_deposit: String,
    pub e_max: u64,
    pub n_opt: u32,
    pub a0: f64,
    pub rho: f64,
    pub tau: f64,
    pub decentralisation_param: f64,
    pub extra_entropy: Option<String>,
    pub protocol_major_ver: u64,
    pub protocol_minor_ver: u64,
    pub min_utxo: String,
    pub min_pool_cost: String,
    pub nonce: String,
    pub cost_models: serde_json::Value,
    pub cost_models_raw: serde_json::Value,
    pub price_mem: f64,
    pub price_step: f64,
    pub max_tx_ex_mem: String,
    pub max_tx_ex_steps: String,
    pub max_block_ex_mem: String,
    pub max_block_ex_steps: String,
    pub max_val_size: String,
    pub collateral_percent: u32,
    pub max_collateral_inputs: u32,
    pub coins_per_utxo_size: String,
    pub coins_per_utxo_word: String,
    pub pvt_motion_no_confidence: f64,
    pub pvt_committee_normal: f64,
    pub pvt_committee_no_confidence: f64,
    pub pvt_hard_fork_initiation: f64,
    pub dvt_motion_no_confidence: f64,
    pub dvt_committee_normal: f64,
    pub dvt_committee_no_confidence: f64,
    pub dvt_update_to_constitution: f64,
    pub dvt_hard_fork_initiation: f64,
    pub dvt_p_p_network_group: f64,
    pub dvt_p_p_economic_group: f64,
    pub dvt_p_p_technical_group: f64,
    pub dvt_p_p_gov_group: f64,
    pub dvt_treasury_withdrawal: f64,
    pub committee_min_size: String,
    pub committee_max_term_length: String,
    pub gov_action_lifetime: String,
    pub gov_action_deposit: String,
    pub drep_deposit: String,
    pub drep_activity: String,
    pub pvtpp_security_group: f64,
    pub pvt_p_p_security_group: f64,
    pub min_fee_ref_script_cost_per_byte: f64,
}

impl From<(u64, ProtocolParams)> for ProtocolParamsRest {
    fn from((epoch, params): (u64, ProtocolParams)) -> Self {
        let shelley = params.shelley.as_ref();
        let shelley_params = shelley.map(|s| &s.protocol_params);
        let alonzo = params.alonzo.as_ref();
        let babbage = params.babbage.as_ref();
        let conway = params.conway.as_ref();

        Self {
            epoch,

            // Shelley params
            min_fee_a: shelley_params.map(|p| p.minfee_a).unwrap_or_default(),
            min_fee_b: shelley_params.map(|p| p.minfee_b).unwrap_or_default(),
            max_block_size: shelley_params.map(|p| p.max_block_body_size).unwrap_or_default(),
            max_tx_size: shelley_params.map(|p| p.max_tx_size).unwrap_or_default(),
            max_block_header_size: shelley_params
                .map(|p| p.max_block_header_size)
                .unwrap_or_default(),
            key_deposit: shelley_params.map(|p| p.key_deposit.to_string()).unwrap_or_default(),
            pool_deposit: shelley_params.map(|p| p.pool_deposit.to_string()).unwrap_or_default(),
            e_max: shelley_params.map(|p| p.pool_retire_max_epoch).unwrap_or_default(),
            n_opt: shelley_params.map(|p| p.stake_pool_target_num).unwrap_or_default(),
            a0: shelley_params
                .map(|p| p.pool_pledge_influence.to_checked_f64("a0").unwrap_or(0.0))
                .unwrap_or_default(),
            rho: shelley_params
                .map(|p| p.monetary_expansion.to_checked_f64("rho").unwrap_or(0.0))
                .unwrap_or_default(),
            tau: shelley_params
                .map(|p| p.treasury_cut.to_checked_f64("tau").unwrap_or(0.0))
                .unwrap_or_default(),
            decentralisation_param: shelley_params
                .map(|p| {
                    p.decentralisation_param.to_checked_f64("decentralization_param").unwrap_or(0.0)
                })
                .unwrap_or_default(),
            extra_entropy: shelley_params
                .map(|p| match &p.extra_entropy {
                    Nonce {
                        tag: NonceVariant::NeutralNonce,
                        ..
                    } => None,
                    Nonce {
                        tag: NonceVariant::Nonce,
                        hash: Some(h),
                    } => Some(hex::encode(h)),
                    _ => None,
                })
                .unwrap_or_default(),
            protocol_major_ver: shelley_params
                .map(|p| p.protocol_version.major)
                .unwrap_or_default(),
            protocol_minor_ver: shelley_params
                .map(|p| p.protocol_version.minor)
                .unwrap_or_default(),
            min_utxo: shelley_params.map(|p| p.min_utxo_value.to_string()).unwrap_or_default(),
            min_pool_cost: shelley_params.map(|p| p.min_pool_cost.to_string()).unwrap_or_default(),
            // TODO: Calculate nonce, store in epoch state, and return here
            nonce: "Not implemented".to_string(),
            cost_models: params.cost_models_json(),
            cost_models_raw: params.cost_models_raw(),

            // Alonzo params
            price_mem: alonzo
                .as_ref()
                .map(|a| a.execution_prices.mem_price.to_checked_f64("price_mem").unwrap_or(0.0))
                .unwrap_or_default(),
            price_step: alonzo
                .as_ref()
                .map(|a| a.execution_prices.step_price.to_checked_f64("price_mem").unwrap_or(0.0))
                .unwrap_or_default(),
            max_tx_ex_mem: alonzo
                .as_ref()
                .map(|a| a.max_tx_ex_units.mem.to_string())
                .unwrap_or_default(),
            max_tx_ex_steps: alonzo
                .as_ref()
                .map(|a| a.max_tx_ex_units.steps.to_string())
                .unwrap_or_default(),
            max_block_ex_mem: alonzo
                .as_ref()
                .map(|a| a.max_block_ex_units.mem.to_string())
                .unwrap_or_default(),
            max_block_ex_steps: alonzo
                .as_ref()
                .map(|a| a.max_block_ex_units.steps.to_string())
                .unwrap_or_default(),
            max_val_size: alonzo.as_ref().map(|a| a.max_value_size.to_string()).unwrap_or_default(),
            collateral_percent: alonzo
                .as_ref()
                .map(|a| a.collateral_percentage)
                .unwrap_or_default(),
            max_collateral_inputs: alonzo
                .as_ref()
                .map(|a| a.max_collateral_inputs)
                .unwrap_or_default(),
            coins_per_utxo_word: alonzo
                .as_ref()
                .map(|a| a.lovelace_per_utxo_word.to_string())
                .unwrap_or_default(),

            // Babbage params
            coins_per_utxo_size: babbage
                .as_ref()
                .map(|b| b.coins_per_utxo_byte.to_string())
                .unwrap_or_default(),

            // Conway params
            pvt_motion_no_confidence: conway
                .as_ref()
                .map(|c| c.pool_voting_thresholds.motion_no_confidence.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            pvt_committee_normal: conway
                .as_ref()
                .map(|c| c.pool_voting_thresholds.committee_normal.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            pvt_committee_no_confidence: conway
                .as_ref()
                .map(|c| c.pool_voting_thresholds.committee_no_confidence.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            pvt_hard_fork_initiation: conway
                .as_ref()
                .map(|c| c.pool_voting_thresholds.hard_fork_initiation.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            dvt_motion_no_confidence: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.motion_no_confidence.to_f64().unwrap_or(0.0))
                .unwrap_or(0.0),
            dvt_committee_normal: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.committee_normal.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            dvt_committee_no_confidence: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.committee_no_confidence.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            dvt_update_to_constitution: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.update_constitution.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            dvt_hard_fork_initiation: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.hard_fork_initiation.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            dvt_p_p_network_group: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.pp_network_group.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            dvt_p_p_economic_group: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.pp_economic_group.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            dvt_p_p_technical_group: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.pp_technical_group.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            dvt_p_p_gov_group: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.pp_governance_group.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            dvt_treasury_withdrawal: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.treasury_withdrawal.to_f64().unwrap_or(0.0))
                .unwrap_or_default(),
            committee_min_size: conway
                .as_ref()
                .map(|c| c.committee_min_size.to_string())
                .unwrap_or_default(),
            committee_max_term_length: conway
                .as_ref()
                .map(|c| c.committee_max_term_length.to_string())
                .unwrap_or_default(),
            gov_action_lifetime: conway
                .as_ref()
                .map(|c| c.gov_action_lifetime.to_string())
                .unwrap_or_default(),
            gov_action_deposit: conway
                .as_ref()
                .map(|c| c.gov_action_deposit.to_string())
                .unwrap_or_default(),
            drep_deposit: conway.as_ref().map(|c| c.d_rep_deposit.to_string()).unwrap_or_default(),
            drep_activity: conway
                .as_ref()
                .map(|c| c.d_rep_activity.to_string())
                .unwrap_or_default(),
            pvtpp_security_group: conway
                .as_ref()
                .map(|c| {
                    c.pool_voting_thresholds.security_voting_threshold.to_f64().unwrap_or_default()
                })
                .unwrap_or_default(),
            pvt_p_p_security_group: conway
                .as_ref()
                .map(|c| {
                    c.pool_voting_thresholds.security_voting_threshold.to_f64().unwrap_or_default()
                })
                .unwrap_or_default(),
            min_fee_ref_script_cost_per_byte: conway
                .as_ref()
                .map(|c| c.min_fee_ref_script_cost_per_byte.to_f64().unwrap_or_default())
                .unwrap_or_default(),
        }
    }
}

use serde_json::{json, Value};

use crate::cost_models::{PLUTUS_V1, PLUTUS_V2, PLUTUS_V3};

/// REST extension trait for Blockfrost-compatible cost model formatting
pub trait ProtocolParamsRestExt {
    fn cost_models_json(&self) -> Value;
    fn cost_models_raw(&self) -> Value;
}

impl ProtocolParamsRestExt for ProtocolParams {
    fn cost_models_json(&self) -> Value {
        let mut map = serde_json::Map::new();

        if let Some(alonzo) = &self.alonzo {
            if let Some(v1) = &alonzo.plutus_v1_cost_model {
                let obj: serde_json::Map<String, Value> = PLUTUS_V1
                    .iter()
                    .zip(v1.as_vec().iter())
                    .map(|(name, val)| (name.to_string(), json!(val)))
                    .collect();
                map.insert("PlutusV1".to_string(), Value::Object(obj));
            }
        }

        if let Some(babbage) = &self.babbage {
            if let Some(v2) = &babbage.plutus_v2_cost_model {
                let obj: serde_json::Map<String, Value> = PLUTUS_V2
                    .iter()
                    .zip(v2.as_vec().iter())
                    .map(|(name, val)| (name.to_string(), json!(val)))
                    .collect();
                map.insert("PlutusV2".to_string(), Value::Object(obj));
            }
        }

        if let Some(conway) = &self.conway {
            let obj: serde_json::Map<String, Value> = PLUTUS_V3
                .iter()
                .zip(conway.plutus_v3_cost_model.as_vec().iter())
                .map(|(name, val)| (name.to_string(), json!(val)))
                .collect();
            map.insert("PlutusV3".to_string(), Value::Object(obj));
        }

        Value::Object(map)
    }

    fn cost_models_raw(&self) -> Value {
        let mut map = serde_json::Map::new();

        if let Some(alonzo) = &self.alonzo {
            if let Some(v1) = &alonzo.plutus_v1_cost_model {
                map.insert("PlutusV1".to_string(), json!(v1.as_vec()));
            }
        }

        if let Some(babbage) = &self.babbage {
            if let Some(v2) = &babbage.plutus_v2_cost_model {
                map.insert("PlutusV2".to_string(), json!(v2.as_vec()));
            }
        }

        if let Some(conway) = &self.conway {
            map.insert(
                "PlutusV3".to_string(),
                json!(conway.plutus_v3_cost_model.as_vec()),
            );
        }

        Value::Object(map)
    }
}
