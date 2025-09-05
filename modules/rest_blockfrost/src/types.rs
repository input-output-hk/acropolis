use acropolis_common::{
    messages::EpochActivityMessage, queries::governance::DRepActionUpdate,
    rest_helper::ToCheckedF64, PoolEpochState, Relay, Vote,
};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

// REST response structure for /epoch
#[derive(Serialize)]
pub struct EpochActivityRest {
    pub epoch: u64,
    pub total_blocks: usize,
    pub total_fees: u64,
    pub vrf_vkey_hashes: Vec<VRFKeyCount>,
}

#[derive(Serialize)]
pub struct VRFKeyCount {
    pub vrf_key_hash: String,
    pub block_count: usize,
}

impl From<EpochActivityMessage> for EpochActivityRest {
    fn from(ea_message: EpochActivityMessage) -> Self {
        Self {
            epoch: ea_message.epoch,
            total_blocks: ea_message.total_blocks,
            total_fees: ea_message.total_fees,
            vrf_vkey_hashes: ea_message
                .vrf_vkey_hashes
                .iter()
                .map(|(key, count)| VRFKeyCount {
                    vrf_key_hash: hex::encode(key),
                    block_count: *count,
                })
                .collect(),
        }
    }
}

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

// REST response structure for /pools/retired and /pools/retiring
#[derive(Serialize)]
pub struct PoolRetirementRest {
    pub pool_id: String,
    pub epoch: u64,
}

// REST response structure for /pools/{pool_id}/history
#[derive(Serialize)]
pub struct PoolEpochStateRest {
    pub epoch: u64,
    pub blocks: u64,
    pub active_stake: String, // u64 in string
    pub active_size: f64,
    pub delegators_count: u64,
    pub rewards: String, // u64 in string
    pub fees: String,    // u64 in string
}

impl From<PoolEpochState> for PoolEpochStateRest {
    fn from(state: PoolEpochState) -> Self {
        Self {
            epoch: state.epoch,
            blocks: state.blocks_minted,
            active_stake: state.active_stake.to_string(),
            active_size: state.active_size.to_checked_f64("active_size").unwrap_or(0.0),
            delegators_count: state.delegators_count,
            rewards: state.pool_reward.to_string(),
            fees: state.spo_reward.to_string(),
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
