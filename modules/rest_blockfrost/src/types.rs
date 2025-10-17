use crate::cost_models::{PLUTUS_V1, PLUTUS_V2, PLUTUS_V3};
use acropolis_common::{
    messages::EpochActivityMessage,
    protocol_params::{Nonce, NonceVariant, ProtocolParams},
    queries::blocks::BlockInfo,
    queries::governance::DRepActionUpdate,
    rest_helper::ToCheckedF64,
    AssetAddressEntry, AssetMetadataStandard, AssetMintRecord, KeyHash, PolicyAsset,
    PoolEpochState, PoolUpdateAction, Relay, TxHash, Vote,
};
use num_traits::ToPrimitive;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{json, Value};
use serde_with::{hex::Hex, serde_as, DisplayFromStr};
use std::collections::HashMap;

// REST response structure for /epoch
#[serde_as]
#[derive(Serialize)]
pub struct EpochActivityRest {
    pub epoch: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub first_block_time: u64,
    pub last_block_time: u64,
    pub block_count: usize,
    pub tx_count: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub output: u128,
    #[serde_as(as = "DisplayFromStr")]
    pub fees: u64,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub active_stake: Option<u64>,
}

impl From<EpochActivityMessage> for EpochActivityRest {
    fn from(ea_message: EpochActivityMessage) -> Self {
        Self {
            epoch: ea_message.epoch,
            start_time: ea_message.epoch_start_time,
            end_time: ea_message.epoch_end_time,
            first_block_time: ea_message.first_block_time,
            last_block_time: ea_message.last_block_time,
            block_count: ea_message.total_blocks,
            tx_count: ea_message.total_txs,
            output: ea_message.total_outputs,
            fees: ea_message.total_fees,
            active_stake: None,
        }
    }
}

// REST response structure for /blocks/latest
#[derive(Serialize)]
pub struct BlockInfoREST(pub BlockInfo);

// REST response structure for /governance/dreps
#[derive(Serialize)]
pub struct DRepsListREST {
    pub drep_id: String,
    pub hex: String,
}

// REST response structure for /governance/dreps/{drep_id}
#[derive(Serialize)]
pub struct DRepInfoREST {
    pub drep_id: String,
    pub hex: String,
    pub amount: String,
    pub active: bool,
    pub active_epoch: Option<u64>,
    pub has_script: bool,
    pub retired: bool,
    pub expired: bool,
    pub last_active_epoch: u64,
}

// REST response structure for /governance/dreps/{drep_id}/delegators
#[allow(dead_code)]
#[derive(Serialize)]
pub struct DRepDelegatorREST {
    pub address: String,
    pub amount: String,
}

// REST response structure for /governance/dreps/{drep_id}/metadata
#[derive(Serialize)]
pub struct DRepMetadataREST {
    pub drep_id: String,
    pub hex: String,
    pub url: String,
    pub hash: String,
    pub json_metadata: Value,
    pub bytes: String,
}

// REST response stucture for /governance/dreps/{drep_id}/updates
#[derive(Serialize)]
pub struct DRepUpdateREST {
    pub tx_hash: String,
    pub cert_index: u64,
    pub action: DRepActionUpdate,
}

// REST response structure for /governance/dreps/{drep_id}/votes
#[derive(Serialize)]
pub struct DRepVoteREST {
    pub tx_hash: String,
    pub cert_index: u32,
    pub vote: Vote,
}

// REST response structure for /governance/proposals
#[allow(dead_code)]
#[derive(Serialize)]
pub struct ProposalsListREST {
    pub tx_hash: String,
    pub cert_index: u64,
    pub governance_type: ProposalTypeREST,
}

#[allow(dead_code)]
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalTypeREST {
    HardForkInitiation,
    NewCommittee,
    NewConstitution,
    InfoAction,
    NoConfidence,
    ParameterChange,
    TreasuryWithdrawals,
}

// REST response structure for /governance/proposals/{tx_hash}/{cert_index}
#[allow(dead_code)]
#[derive(Serialize)]
pub struct ProposalInfoREST {
    pub tx_hash: String,
    pub cert_index: u64,
    pub governance_type: ProposalTypeREST,
    pub deposit: u64,
    pub return_address: String,
    pub governance_description: String,
    pub ratified_epoch: Option<u64>,
    pub enacted_epoch: Option<u64>,
    pub dropped_epoch: Option<u64>,
    pub expired_epoch: Option<u64>,
    pub expiration: u64,
}

// REST response structure for /governance/proposals/{tx_hash}/{cert_index}/parameters
#[allow(dead_code)]
#[derive(Serialize)]
pub struct ProposalParametersREST {
    pub tx_hash: String,
    pub cert_index: u64,
    pub parameters: ParametersREST,
}

#[derive(Serialize)]
pub struct ParametersREST {
    pub epoch: Option<u64>,
    pub min_fee_a: Option<u64>,
    pub min_fee_b: Option<u64>,
    pub max_block_size: Option<u64>,
    pub max_tx_size: Option<u64>,
    pub max_block_header_size: Option<u64>,
    pub key_deposit: Option<String>,
    pub pool_deposit: Option<String>,
    pub e_max: Option<u64>,
    pub n_opt: Option<u64>,
    pub a0: Option<f64>,
    pub rho: Option<f64>,
    pub tau: Option<f64>,
    pub decentralisation_param: Option<f64>,
    pub extra_entropy: Option<String>,
    pub protocol_major_ver: Option<u64>,
    pub protocol_minor_ver: Option<u64>,
    pub min_utxo: Option<String>,
    pub min_pool_cost: Option<String>,
    pub cost_models: Option<HashMap<String, Vec<u64>>>,
    pub price_mem: Option<f64>,
    pub price_step: Option<f64>,
    pub max_tx_ex_mem: Option<String>,
    pub max_tx_ex_steps: Option<String>,
    pub max_block_ex_mem: Option<String>,
    pub max_block_ex_steps: Option<String>,
    pub max_val_size: Option<String>,
    pub collateral_percent: Option<u64>,
    pub max_collateral_inputs: Option<u64>,
    pub coins_per_utxo_size: Option<String>,
    pub coins_per_utxo_word: Option<String>,
    pub pvt_motion_no_confidence: Option<u64>,
    pub pvt_committee_normal: Option<u64>,
    pub pvt_committee_no_confidence: Option<u64>,
    pub pvt_hard_fork_initation: Option<u64>,
    pub dvt_motion_no_confidence: Option<u64>,
    pub dvt_committee_normal: Option<u64>,
    pub dvt_committee_no_confidence: Option<u64>,
    pub dvt_update_to_constitution: Option<u64>,
    pub dvt_hard_fork_initation: Option<u64>,
    pub dvt_p_p_network_group: Option<u64>,
    pub dvt_p_p_economic_group: Option<u64>,
    pub dvt_p_p_technical_group: Option<u64>,
    pub dvt_p_p_gov_group: Option<u64>,
    pub dvt_treasury_withdrawal: Option<u64>,
    pub committee_min_size: Option<String>,
    pub committee_max_term_length: Option<String>,
    pub gov_action_lifetime: Option<String>,
    pub gov_action_deposit: Option<String>,
    pub drep_deposit: Option<String>,
    pub drep_activity: Option<String>,
    pub pvtpp_security_group: Option<u64>,
    pub pvt_p_p_security_group: Option<u64>,
    pub min_fee_ref_script_cost_per_byte: Option<u64>,
}

// REST response structure for /governance/proposals/{tx_hash}/{cert_index}/withdrawals
#[allow(dead_code)]
#[derive(Serialize)]
pub struct ProposalWithdrawalsREST {
    pub stake_address: String,
    pub amount: String,
}

// REST response structure for /governance/proposals/{tx_hash}/{cert_index}/votes
#[derive(Serialize)]
pub struct ProposalVoteREST {
    pub tx_hash: String,
    pub cert_index: u8,
    pub voter_role: VoterRoleREST,
    pub voter: String,
    pub vote: Vote,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoterRoleREST {
    ConstitutionalCommittee,
    Drep,
    Spo,
}

// REST response structure for /governance/proposals/{tx_hash}/{cert_index}/metadata
#[allow(dead_code)]
#[derive(Serialize)]
pub struct ProposalMetadataREST {
    pub tx_hash: String,
    pub cert_index: u64,
    pub url: String,
    pub hash: String,
    pub json_meta_data: Value,
    pub bytes: String,
}

// RET response structure for /pools/extended
#[serde_as]
#[derive(Serialize)]
pub struct PoolExtendedRest {
    pub pool_id: String,
    #[serde_as(as = "Hex")]
    pub hex: Vec<u8>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub active_stake: Option<u64>,
    #[serde_as(as = "DisplayFromStr")]
    pub live_stake: u64,
    pub blocks_minted: u64,
    pub live_saturation: Decimal,
    #[serde_as(as = "DisplayFromStr")]
    pub declared_pledge: u64,
    pub margin_cost: f32,
    #[serde_as(as = "DisplayFromStr")]
    pub fixed_cost: u64,
}

// REST response structure for /pools/retired and /pools/retiring
#[derive(Serialize)]
pub struct PoolRetirementRest {
    pub pool_id: String,
    pub epoch: u64,
}

// REST response structure for /pools/{pool_id}/history
#[serde_as]
#[derive(Serialize)]
pub struct PoolEpochStateRest {
    pub epoch: u64,
    pub blocks: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub active_stake: u64,
    pub active_size: f64,
    #[serde_as(as = "DisplayFromStr")]
    pub delegators_count: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub rewards: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub fees: u64,
}

impl From<PoolEpochState> for PoolEpochStateRest {
    fn from(state: PoolEpochState) -> Self {
        Self {
            epoch: state.epoch,
            blocks: state.blocks_minted,
            active_stake: state.active_stake,
            active_size: state.active_size.to_checked_f64("active_size").unwrap_or(0.0),
            delegators_count: state.delegators_count,
            rewards: state.pool_reward,
            fees: state.spo_reward,
        }
    }
}

// REST response structure for /pools/{pool_id}/metadata
#[derive(Serialize)]
pub struct PoolMetadataRest {
    pub pool_id: String,
    pub hex: String,
    pub url: String,
    pub hash: String,
    pub ticker: String,
    pub name: String,
    pub description: String,
    pub homepage: String,
}

// REST response structure for /pools/{pool_id}/delegators
#[derive(Serialize)]
pub struct PoolDelegatorRest {
    // stake bech32
    pub address: String,
    // live stake
    pub live_stake: String,
}

// REST response structure for /pools/{pool_id}/relays
#[derive(Serialize)]
pub struct PoolRelayRest {
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub dns: Option<String>,
    pub dns_srv: Option<String>,
    pub port: u16,
}

impl From<Relay> for PoolRelayRest {
    fn from(value: Relay) -> Self {
        //todo: port is required on BlockFrost. Need a default value, if not provided
        let default_port = 3001;

        match value {
            Relay::SingleHostAddr(s) => PoolRelayRest {
                ipv4: s.ipv4.map(|bytes| {
                    let ipv4_addr = std::net::Ipv4Addr::from(bytes);
                    format!("{:?}", ipv4_addr)
                }),
                ipv6: s.ipv6.map(|bytes| {
                    let ipv6_addr = std::net::Ipv6Addr::from(bytes);
                    format!("{:?}", ipv6_addr)
                }),
                dns: None,
                dns_srv: None,
                port: s.port.unwrap_or(default_port),
            },
            Relay::SingleHostName(s) => PoolRelayRest {
                ipv4: None,
                ipv6: None,
                dns: Some(s.dns_name),
                dns_srv: None,
                port: s.port.unwrap_or(default_port),
            },
            Relay::MultiHostName(m) => PoolRelayRest {
                ipv4: None,
                ipv6: None,
                dns: None,
                dns_srv: Some(m.dns_name),
                port: default_port,
            },
        }
    }
}

// REST response structure for /pools/{pool_id}/updates
#[serde_as]
#[derive(Serialize)]
pub struct PoolUpdateEventRest {
    #[serde_as(as = "Hex")]
    pub tx_hash: TxHash,
    pub cert_index: u64,
    pub action: PoolUpdateAction,
}

// REST response structure for /pools/{pool_id}/votes
#[serde_as]
#[derive(Serialize)]
pub struct PoolVoteRest {
    #[serde_as(as = "Hex")]
    pub tx_hash: TxHash,
    pub vote_index: u32,
    pub vote: Vote,
}

// REST response structure for /pools/{pool_id}
#[serde_as]
#[derive(Serialize)]
pub struct PoolInfoRest {
    pub pool_id: String,
    #[serde_as(as = "Hex")]
    pub hex: KeyHash,
    #[serde_as(as = "Hex")]
    pub vrf_key: KeyHash,
    pub blocks_minted: u64,
    pub blocks_epoch: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub live_stake: u64,
    pub live_size: Decimal,
    pub live_saturation: Decimal,
    pub live_delegators: u64,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub active_stake: Option<u64>,
    pub active_size: Option<f64>,
    #[serde_as(as = "DisplayFromStr")]
    pub declared_pledge: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub live_pledge: u64,
    pub margin_cost: f32,
    #[serde_as(as = "DisplayFromStr")]
    pub fixed_cost: u64,
    pub reward_account: String,
    pub pool_owners: Vec<String>,
    #[serde_as(as = "Option<Vec<Hex>>")]
    pub registration: Option<Vec<TxHash>>,
    #[serde_as(as = "Option<Vec<Hex>>")]
    pub retirement: Option<Vec<TxHash>>,
}

// REST response structure for protocol params
#[derive(Serialize)]
pub struct ProtocolParamsRest {
    pub epoch: u64,
    pub min_fee_a: Option<u32>,
    pub min_fee_b: Option<u32>,
    pub max_block_size: Option<u32>,
    pub max_tx_size: Option<u32>,
    pub max_block_header_size: Option<u32>,
    pub key_deposit: Option<String>,
    pub pool_deposit: Option<String>,
    pub e_max: Option<u64>,
    pub n_opt: Option<u32>,
    pub a0: Option<f64>,
    pub rho: Option<f64>,
    pub tau: Option<f64>,
    pub decentralisation_param: Option<f64>,
    pub extra_entropy: Option<String>,
    pub protocol_major_ver: Option<u64>,
    pub protocol_minor_ver: Option<u64>,
    pub min_utxo: Option<String>,
    pub min_pool_cost: Option<String>,
    pub nonce: Option<String>,
    pub cost_models: Option<serde_json::Value>,
    pub cost_models_raw: Option<serde_json::Value>,
    pub price_mem: Option<f64>,
    pub price_step: Option<f64>,
    pub max_tx_ex_mem: Option<String>,
    pub max_tx_ex_steps: Option<String>,
    pub max_block_ex_mem: Option<String>,
    pub max_block_ex_steps: Option<String>,
    pub max_val_size: Option<String>,
    pub collateral_percent: Option<u32>,
    pub max_collateral_inputs: Option<u32>,
    pub coins_per_utxo_size: Option<String>,
    pub coins_per_utxo_word: Option<String>,
    pub pvt_motion_no_confidence: Option<f64>,
    pub pvt_committee_normal: Option<f64>,
    pub pvt_committee_no_confidence: Option<f64>,
    pub pvt_hard_fork_initiation: Option<f64>,
    pub dvt_motion_no_confidence: Option<f64>,
    pub dvt_committee_normal: Option<f64>,
    pub dvt_committee_no_confidence: Option<f64>,
    pub dvt_update_to_constitution: Option<f64>,
    pub dvt_hard_fork_initiation: Option<f64>,
    pub dvt_p_p_network_group: Option<f64>,
    pub dvt_p_p_economic_group: Option<f64>,
    pub dvt_p_p_technical_group: Option<f64>,
    pub dvt_p_p_gov_group: Option<f64>,
    pub dvt_treasury_withdrawal: Option<f64>,
    pub committee_min_size: Option<String>,
    pub committee_max_term_length: Option<String>,
    pub gov_action_lifetime: Option<String>,
    pub gov_action_deposit: Option<String>,
    pub drep_deposit: Option<String>,
    pub drep_activity: Option<String>,
    pub pvtpp_security_group: Option<f64>,
    pub pvt_p_p_security_group: Option<f64>,
    pub min_fee_ref_script_cost_per_byte: Option<f64>,
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
            min_fee_a: shelley_params.map(|p| p.minfee_a),
            min_fee_b: shelley_params.map(|p| p.minfee_b),
            max_block_size: shelley_params.map(|p| p.max_block_body_size),
            max_tx_size: shelley_params.map(|p| p.max_tx_size),
            max_block_header_size: shelley_params.map(|p| p.max_block_header_size),
            key_deposit: shelley_params.map(|p| p.key_deposit.to_string()),
            pool_deposit: shelley_params.map(|p| p.pool_deposit.to_string()),
            e_max: shelley_params.map(|p| p.pool_retire_max_epoch),
            n_opt: shelley_params.map(|p| p.stake_pool_target_num),
            a0: shelley_params.and_then(|p| p.pool_pledge_influence.to_checked_f64("a0").ok()),
            rho: shelley_params.and_then(|p| p.monetary_expansion.to_checked_f64("rho").ok()),
            tau: shelley_params.and_then(|p| p.treasury_cut.to_checked_f64("tau").ok()),
            decentralisation_param: shelley_params.and_then(|p| {
                p.decentralisation_param.to_checked_f64("decentralisation_param").ok()
            }),
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
            protocol_major_ver: shelley_params.map(|p| p.protocol_version.major),
            protocol_minor_ver: shelley_params.map(|p| p.protocol_version.minor),
            min_utxo: shelley_params.map(|p| p.min_utxo_value.to_string()),
            min_pool_cost: shelley_params.map(|p| p.min_pool_cost.to_string()),
            // TODO: Calculate nonce, store in epoch state, and return here
            nonce: Some("Not implemented".to_string()),
            cost_models: Some(params.cost_models_json()),
            cost_models_raw: Some(params.cost_models_raw()),

            // Alonzo params
            price_mem: alonzo
                .and_then(|a| a.execution_prices.mem_price.to_checked_f64("price_mem").ok()),
            price_step: alonzo
                .and_then(|a| a.execution_prices.step_price.to_checked_f64("price_step").ok()),
            max_tx_ex_mem: alonzo.as_ref().map(|a| a.max_tx_ex_units.mem.to_string()),
            max_tx_ex_steps: alonzo.as_ref().map(|a| a.max_tx_ex_units.steps.to_string()),
            max_block_ex_mem: alonzo.as_ref().map(|a| a.max_block_ex_units.mem.to_string()),
            max_block_ex_steps: alonzo.as_ref().map(|a| a.max_block_ex_units.steps.to_string()),
            max_val_size: alonzo.as_ref().map(|a| a.max_value_size.to_string()),
            collateral_percent: alonzo.as_ref().map(|a| a.collateral_percentage),
            max_collateral_inputs: alonzo.as_ref().map(|a| a.max_collateral_inputs),
            coins_per_utxo_word: alonzo.as_ref().map(|a| a.lovelace_per_utxo_word.to_string()),
            // Babbage params
            coins_per_utxo_size: babbage.as_ref().map(|b| b.coins_per_utxo_byte.to_string()),

            // Conway params
            pvt_motion_no_confidence: conway
                .as_ref()
                .map(|c| c.pool_voting_thresholds.motion_no_confidence.to_f64().unwrap_or(0.0)),
            pvt_committee_normal: conway
                .as_ref()
                .map(|c| c.pool_voting_thresholds.committee_normal.to_f64().unwrap_or(0.0)),
            pvt_committee_no_confidence: conway
                .as_ref()
                .map(|c| c.pool_voting_thresholds.committee_no_confidence.to_f64().unwrap_or(0.0)),
            pvt_hard_fork_initiation: conway
                .as_ref()
                .map(|c| c.pool_voting_thresholds.hard_fork_initiation.to_f64().unwrap_or(0.0)),
            dvt_motion_no_confidence: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.motion_no_confidence.to_f64().unwrap_or(0.0)),
            dvt_committee_normal: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.committee_normal.to_f64().unwrap_or(0.0)),
            dvt_committee_no_confidence: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.committee_no_confidence.to_f64().unwrap_or(0.0)),
            dvt_update_to_constitution: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.update_constitution.to_f64().unwrap_or(0.0)),
            dvt_hard_fork_initiation: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.hard_fork_initiation.to_f64().unwrap_or(0.0)),
            dvt_p_p_network_group: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.pp_network_group.to_f64().unwrap_or(0.0)),
            dvt_p_p_economic_group: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.pp_economic_group.to_f64().unwrap_or(0.0)),
            dvt_p_p_technical_group: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.pp_technical_group.to_f64().unwrap_or(0.0)),
            dvt_p_p_gov_group: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.pp_governance_group.to_f64().unwrap_or(0.0)),
            dvt_treasury_withdrawal: conway
                .as_ref()
                .map(|c| c.d_rep_voting_thresholds.treasury_withdrawal.to_f64().unwrap_or(0.0)),
            committee_min_size: conway.as_ref().map(|c| c.committee_min_size.to_string()),
            committee_max_term_length: conway
                .as_ref()
                .map(|c| c.committee_max_term_length.to_string()),
            gov_action_lifetime: conway.as_ref().map(|c| c.gov_action_lifetime.to_string()),
            gov_action_deposit: conway.as_ref().map(|c| c.gov_action_deposit.to_string()),
            drep_deposit: conway.as_ref().map(|c| c.d_rep_deposit.to_string()),
            drep_activity: conway.as_ref().map(|c| c.d_rep_activity.to_string()),
            pvtpp_security_group: conway.as_ref().map(|c| {
                c.pool_voting_thresholds.security_voting_threshold.to_f64().unwrap_or_default()
            }),
            pvt_p_p_security_group: conway.as_ref().map(|c| {
                c.pool_voting_thresholds.security_voting_threshold.to_f64().unwrap_or_default()
            }),
            min_fee_ref_script_cost_per_byte: conway
                .as_ref()
                .map(|c| c.min_fee_ref_script_cost_per_byte.to_f64().unwrap_or_default()),
        }
    }
}

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

#[derive(Serialize)]
pub struct AssetInfoRest {
    pub asset: String,
    pub policy_id: String,
    pub asset_name: String,
    pub fingerprint: String,
    pub quantity: String,
    pub initial_mint_tx_hash: String,
    pub mint_or_burn_count: u64,
    pub onchain_metadata: Option<Value>,
    pub onchain_metadata_standard: Option<AssetMetadataStandard>,
    pub onchain_metadata_extra: Option<String>,
    pub metadata: Option<AssetMetadata>,
}

#[derive(Serialize, Clone)]
pub struct AssetMetadata {
    pub name: String,
    pub description: String,
    pub ticker: Option<String>,
    pub url: Option<String>,
    pub logo: Option<String>,
    pub decimals: Option<u8>,
}

#[derive(Debug, Serialize)]
pub struct AssetMintRecordRest {
    tx_hash: String,
    amount: String,
    action: String,
}

impl From<&AssetMintRecord> for AssetMintRecordRest {
    fn from(record: &AssetMintRecord) -> Self {
        let action = if !record.burn {
            "minted".to_string()
        } else {
            "burned".to_string()
        };

        AssetMintRecordRest {
            tx_hash: "transaction_state not yet implemented".to_string(),
            amount: record.amount.to_string(),
            action,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PolicyAssetRest {
    asset: String,
    quantity: String,
}

impl From<&PolicyAsset> for PolicyAssetRest {
    fn from(asset: &PolicyAsset) -> Self {
        let asset_hex = format!(
            "{}{}",
            hex::encode(asset.policy),
            hex::encode(asset.name.as_slice())
        );

        PolicyAssetRest {
            asset: asset_hex,
            quantity: asset.quantity.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AssetTransactionRest {
    pub tx_hash: String, // Requires a query to transactions state which is not yet implemented
    pub tx_index: u16,
    pub block_height: u32,
    pub block_time: String, // Change to u64 when transactions state is implemented
}

#[derive(Debug, Serialize)]
pub struct AssetAddressRest {
    pub address: String,
    pub quantity: String,
}

impl TryFrom<&AssetAddressEntry> for AssetAddressRest {
    type Error = anyhow::Error;

    fn try_from(entry: &AssetAddressEntry) -> Result<Self, Self::Error> {
        Ok(AssetAddressRest {
            address: entry.address.to_string()?,
            quantity: entry.quantity.to_string(),
        })
    }
}
