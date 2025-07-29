//! REST handlers for Acropolis Parameters State module
use crate::state::State;
use acropolis_common::{
    messages::RESTResponse, rest_helper::ToCheckedF64, AlonzoParams, Anchor, BlockVersionData,
    ByronParams, Committee, Constitution, ConwayParams, DRepVotingThresholds, ExUnitPrices,
    ExUnits, NetworkId, Nonce, NonceVariant, PoolVotingThresholds, ProtocolConsts, ProtocolParams,
    ProtocolVersion, ShelleyParams, ShelleyProtocolParams,
};
use anyhow::Result;
use pallas::ledger::primitives::CostModel;
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

/// REST response structure for protocol parameters
#[derive(Serialize)]
pub struct ProtocolParametersRest {
    /// Epoch these parameters became effective
    pub active_epoch: u64,

    /// Protocol parameters grouped by era.
    pub params: EraParametersRest,
}

#[derive(Serialize)]
pub struct EraParametersRest {
    pub byron: Option<ByronParamsRest>,
    pub shelley: Option<ShelleyParamsRest>,
    pub alonzo: Option<AlonzoParamsRest>,
    pub conway: Option<ConwayParamsRest>,
}

#[derive(Serialize)]
pub struct ByronParamsRest {
    pub block_version_data: BlockVersionData,
    /// fts_seed hex encoded
    pub fts_seed: String,
    pub protocol_consts: ProtocolConsts,
    pub start_time: u64,
}

#[derive(Serialize)]
pub struct ShelleyParamsRest {
    pub active_slots_coeff: f32,
    pub epoch_length: u32,
    pub max_kes_evolutions: u32,
    pub max_lovelace_supply: u64,
    pub network_id: NetworkId,
    pub network_magic: u32,
    /// uses ShelleyProtocolParamsRest
    pub protocol_params: ShelleyProtocolParamsRest,
    pub security_param: u32,
    pub slot_length: u32,
    pub slots_per_kes_period: u32,
    /// UNIX timestamp
    pub system_start: u64,
    pub update_quorum: u32,
}

#[derive(Serialize)]
pub struct ShelleyProtocolParamsRest {
    pub protocol_version: ProtocolVersion,
    pub max_tx_size: u32,
    pub max_block_body_size: u32,
    pub max_block_header_size: u32,
    pub key_deposit: u64,
    pub min_utxo_value: u64,

    pub minfee_a: u32,
    pub minfee_b: u32,
    pub pool_deposit: u64,

    pub stake_pool_target_num: u32,
    pub min_pool_cost: u64,

    pub pool_retire_max_epoch: u64,
    /// hex encoded hash
    pub extra_entropy: NonceRest,
    /// decentralisation_param as float
    pub decentralisation_param: f64,

    /// monetary_expansion as float
    pub monetary_expansion: f64,

    /// treasury_cut as float
    pub treasury_cut: f64,

    /// pool_pledge_influence as float
    pub pool_pledge_influence: f64,
}

#[derive(Serialize)]
pub struct NonceRest {
    pub tag: NonceVariant,
    pub hash: Option<String>,
}

#[derive(Serialize)]
pub struct AlonzoParamsRest {
    pub lovelace_per_utxo_word: u64,
    /// uses ExUnitPricesRest
    pub execution_prices: ExUnitPricesRest,
    pub max_tx_ex_units: ExUnits,
    pub max_block_ex_units: ExUnits,
    pub max_value_size: u32,
    pub collateral_percentage: u32,
    pub max_collateral_inputs: u32,
    pub plutus_v1_cost_model: Option<CostModel>,
    pub plutus_v2_cost_model: Option<CostModel>,
}

#[derive(Serialize)]
pub struct ExUnitPricesRest {
    /// mem_price as float
    pub mem_price: f64,
    /// step_price as float
    pub step_price: f64,
}

#[derive(Serialize)]
pub struct ConwayParamsRest {
    /// Uses PoolVotingThresholdsRest
    pub pool_voting_thresholds: PoolVotingThresholdsRest,
    /// Uses DRepVotingThresholdsRest
    pub d_rep_voting_thresholds: DRepVotingThresholdsRest,
    pub committee_min_size: u64,
    pub committee_max_term_length: u32,
    pub gov_action_lifetime: u32,
    pub gov_action_deposit: u64,
    pub d_rep_deposit: u64,
    pub d_rep_activity: u32,
    /// min_fee_ref_script_cost_per_byte as float
    pub min_fee_ref_script_cost_per_byte: f64,
    pub plutus_v3_cost_model: CostModel,
    /// Uses ConstitutionRest
    pub constitution: ConstitutionRest,
    /// Uses CommitteeRest
    pub committee: CommitteeRest,
}

#[derive(serde::Serialize)]
pub struct PoolVotingThresholdsRest {
    // All fields as float
    pub motion_no_confidence: f64,
    pub committee_normal: f64,
    pub committee_no_confidence: f64,
    pub hard_fork_initiation: f64,
    pub security_voting_threshold: f64,
}

#[derive(serde::Serialize)]
pub struct DRepVotingThresholdsRest {
    // All fields as float
    pub motion_no_confidence: f64,
    pub committee_normal: f64,
    pub committee_no_confidence: f64,
    pub update_constitution: f64,
    pub hard_fork_initiation: f64,
    pub pp_network_group: f64,
    pub pp_economic_group: f64,
    pub pp_technical_group: f64,
    pub pp_governance_group: f64,
    pub treasury_withdrawal: f64,
}

#[derive(serde::Serialize)]
pub struct ConstitutionRest {
    pub anchor: Anchor,
    /// guardrail_script hex encoded
    pub guardrail_script: Option<String>,
}

#[derive(serde::Serialize)]
pub struct CommitteeRest {
    /// members key expresses as string
    pub members: HashMap<String, u64>,
    /// threshold as float
    pub threshold: f64,
}

/// REST handler for /epoch/parameters — returns current live protocol parameters
pub async fn handle_current(state: Arc<Mutex<State>>) -> Result<RESTResponse> {
    let lock = state.lock().await;
    let params = lock.current_params.get_params();
    let active_epoch = lock.active_epoch;

    let response = match ProtocolParametersRest::try_from((&active_epoch, &params)) {
        Ok(r) => r,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!(
                    "Internal server error while retrieving parameters for current epoch: {e}"
                ),
            ))
        }
    };

    let json = match serde_json::to_string(&response) {
        Ok(j) => j,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!(
                    "Internal server error while retrieving parameters for current epoch: {e}"
                ),
            ))
        }
    };

    Ok(RESTResponse::with_json(200, &json))
}

/// REST handler for /epochs/{epoch_number}/parameters — returns parameters for the closest prior epoch
pub async fn handle_historical(state: Arc<Mutex<State>>, epoch: String) -> Result<RESTResponse> {
    let parsed_epoch = match epoch.parse::<u64>() {
        Ok(v) => v,
        Err(_) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid epoch number: {epoch}"),
            ));
        }
    };

    let lock = state.lock().await;

    match &lock.parameter_history {
        Some(history) => match history.range(..=parsed_epoch).next_back() {
            Some((epoch_found, msg)) => match ProtocolParametersRest::try_from((epoch_found, &msg.params)) {
                Ok(response) => match serde_json::to_string(&response) {
                    Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                    Err(e) => Ok(RESTResponse::with_text(
                        500,
                        &format!("Internal server error while retrieving parameters for epoch {epoch_found}: {e}"),
                    )),
                },
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while retrieving parameters for epoch {epoch_found}: {e}"),
                )),
            },
            None => Ok(RESTResponse::with_text(404, "Epoch not found")),
        },
        None => Ok(RESTResponse::with_text(
            501,
            "Historical parameter storage not enabled",
        )),
    }
}

/// ProtocolParametersRest helper functions
impl TryFrom<(&u64, &ProtocolParams)> for ProtocolParametersRest {
    type Error = anyhow::Error;

    fn try_from((epoch, params): (&u64, &ProtocolParams)) -> Result<Self> {
        Ok(Self {
            active_epoch: *epoch,
            params: EraParametersRest::try_from(params)?,
        })
    }
}

impl TryFrom<&ProtocolParams> for EraParametersRest {
    type Error = anyhow::Error;

    fn try_from(params: &ProtocolParams) -> Result<Self> {
        Ok(Self {
            byron: params.byron.as_ref().map(ByronParamsRest::from),
            shelley: match &params.shelley {
                Some(shelley) => Some(ShelleyParamsRest::try_from(shelley)?),
                None => None,
            },
            alonzo: match &params.alonzo {
                Some(alonzo) => Some(AlonzoParamsRest::try_from(alonzo)?),
                None => None,
            },
            conway: match &params.conway {
                Some(conway) => Some(ConwayParamsRest::try_from(conway)?),
                None => None,
            },
        })
    }
}

/// Conversions from stored objects to REST objects
impl From<&ByronParams> for ByronParamsRest {
    fn from(params: &ByronParams) -> Self {
        Self {
            block_version_data: params.block_version_data.clone(),
            fts_seed: hex::encode(params.fts_seed.as_ref().unwrap_or(&vec![])),
            protocol_consts: params.protocol_consts.clone(),
            start_time: params.start_time,
        }
    }
}

impl TryFrom<&ShelleyParams> for ShelleyParamsRest {
    type Error = anyhow::Error;

    fn try_from(params: &ShelleyParams) -> Result<Self> {
        Ok(Self {
            active_slots_coeff: params.active_slots_coeff,
            epoch_length: params.epoch_length,
            max_kes_evolutions: params.max_kes_evolutions,
            max_lovelace_supply: params.max_lovelace_supply,
            network_id: params.network_id.clone(),
            network_magic: params.network_magic,
            protocol_params: (&params.protocol_params).try_into()?,
            security_param: params.security_param,
            slot_length: params.slot_length,
            slots_per_kes_period: params.slots_per_kes_period,
            system_start: params.system_start.timestamp() as u64,
            update_quorum: params.update_quorum,
        })
    }
}

impl TryFrom<&ShelleyProtocolParams> for ShelleyProtocolParamsRest {
    type Error = anyhow::Error;

    fn try_from(params: &ShelleyProtocolParams) -> Result<Self> {
        Ok(Self {
            protocol_version: params.protocol_version.clone(),
            max_tx_size: params.max_tx_size,
            max_block_body_size: params.max_block_body_size,
            max_block_header_size: params.max_block_header_size,
            key_deposit: params.key_deposit,
            min_utxo_value: params.min_utxo_value,
            minfee_a: params.minfee_a,
            minfee_b: params.minfee_b,
            pool_deposit: params.pool_deposit,
            stake_pool_target_num: params.stake_pool_target_num,
            min_pool_cost: params.min_pool_cost,
            pool_retire_max_epoch: params.pool_retire_max_epoch,
            extra_entropy: NonceRest::from(&params.extra_entropy),
            decentralisation_param: params
                .decentralisation_param
                .to_checked_f64("decentralisation_param")?,
            monetary_expansion: params.monetary_expansion.to_checked_f64("monetary_expansion")?,
            treasury_cut: params.treasury_cut.to_checked_f64("treasury_cut")?,
            pool_pledge_influence: params
                .pool_pledge_influence
                .to_checked_f64("pool_pledge_influence")?,
        })
    }
}

impl From<&Nonce> for NonceRest {
    fn from(nonce: &Nonce) -> Self {
        Self {
            tag: nonce.tag.clone(),
            hash: nonce.hash.as_ref().map(hex::encode),
        }
    }
}

impl TryFrom<&AlonzoParams> for AlonzoParamsRest {
    type Error = anyhow::Error;

    fn try_from(src: &AlonzoParams) -> Result<Self> {
        Ok(Self {
            lovelace_per_utxo_word: src.lovelace_per_utxo_word,
            execution_prices: (&src.execution_prices).try_into()?,
            max_tx_ex_units: src.max_tx_ex_units.clone(),
            max_block_ex_units: src.max_block_ex_units.clone(),
            max_value_size: src.max_value_size,
            collateral_percentage: src.collateral_percentage,
            max_collateral_inputs: src.max_collateral_inputs,
            plutus_v1_cost_model: src.plutus_v1_cost_model.clone(),
            plutus_v2_cost_model: src.plutus_v2_cost_model.clone(),
        })
    }
}

impl TryFrom<&ExUnitPrices> for ExUnitPricesRest {
    type Error = anyhow::Error;

    fn try_from(src: &ExUnitPrices) -> Result<Self> {
        Ok(Self {
            mem_price: src.mem_price.to_checked_f64("mem_price")?,
            step_price: src.step_price.to_checked_f64("step_price")?,
        })
    }
}

impl TryFrom<&ConwayParams> for ConwayParamsRest {
    type Error = anyhow::Error;

    fn try_from(src: &ConwayParams) -> Result<Self> {
        Ok(Self {
            pool_voting_thresholds: (&src.pool_voting_thresholds).try_into()?,
            d_rep_voting_thresholds: (&src.d_rep_voting_thresholds).try_into()?,
            committee_min_size: src.committee_min_size,
            committee_max_term_length: src.committee_max_term_length,
            gov_action_lifetime: src.gov_action_lifetime,
            gov_action_deposit: src.gov_action_deposit,
            d_rep_deposit: src.d_rep_deposit,
            d_rep_activity: src.d_rep_activity,
            min_fee_ref_script_cost_per_byte: src
                .min_fee_ref_script_cost_per_byte
                .to_checked_f64("min_fee_ref_script_cost_per_byte")?,
            plutus_v3_cost_model: src.plutus_v3_cost_model.clone(),
            constitution: (&src.constitution).try_into()?,
            committee: (&src.committee).try_into()?,
        })
    }
}

impl TryFrom<&PoolVotingThresholds> for PoolVotingThresholdsRest {
    type Error = anyhow::Error;

    fn try_from(src: &PoolVotingThresholds) -> Result<Self, Self::Error> {
        Ok(Self {
            motion_no_confidence: src
                .motion_no_confidence
                .to_checked_f64("motion_no_confidence")?,
            committee_normal: src.committee_normal.to_checked_f64("committee_normal")?,
            committee_no_confidence: src
                .committee_no_confidence
                .to_checked_f64("committee_no_confidence")?,
            hard_fork_initiation: src
                .hard_fork_initiation
                .to_checked_f64("hard_fork_initiation")?,
            security_voting_threshold: src
                .security_voting_threshold
                .to_checked_f64("security_voting_threshold")?,
        })
    }
}

impl TryFrom<&DRepVotingThresholds> for DRepVotingThresholdsRest {
    type Error = anyhow::Error;

    fn try_from(src: &DRepVotingThresholds) -> Result<Self, Self::Error> {
        Ok(Self {
            motion_no_confidence: src
                .motion_no_confidence
                .to_checked_f64("motion_no_confidence")?,
            committee_normal: src.committee_normal.to_checked_f64("committee_normal")?,
            committee_no_confidence: src
                .committee_no_confidence
                .to_checked_f64("committee_no_confidence")?,
            update_constitution: src.update_constitution.to_checked_f64("update_constitution")?,
            hard_fork_initiation: src
                .hard_fork_initiation
                .to_checked_f64("hard_fork_initiation")?,
            pp_network_group: src.pp_network_group.to_checked_f64("pp_network_group")?,
            pp_economic_group: src.pp_economic_group.to_checked_f64("pp_economic_group")?,
            pp_technical_group: src.pp_technical_group.to_checked_f64("pp_technical_group")?,
            pp_governance_group: src.pp_governance_group.to_checked_f64("pp_governance_group")?,
            treasury_withdrawal: src.treasury_withdrawal.to_checked_f64("treasury_withdrawal")?,
        })
    }
}

impl From<&Constitution> for ConstitutionRest {
    fn from(src: &Constitution) -> Self {
        Self {
            anchor: src.anchor.clone(),
            guardrail_script: src
                .guardrail_script
                .as_ref()
                .map(|script_hash| hex::encode(script_hash)),
        }
    }
}

impl TryFrom<&Committee> for CommitteeRest {
    type Error = anyhow::Error;

    fn try_from(src: &Committee) -> Result<Self> {
        Ok(Self {
            members: src
                .members
                .iter()
                .map(|(cred, amount)| (cred.to_json_string(), *amount))
                .collect(),
            threshold: src.threshold.to_checked_f64("threshold")?,
        })
    }
}
