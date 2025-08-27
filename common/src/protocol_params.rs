use crate::{
    rational_number::{ChameleonFraction, RationalNumber},
    Anchor, Committee, ExUnitPrices, ExUnits, GovActionId, Lovelace, ScriptHash,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

#[derive(Debug, Default, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParams {
    pub byron: Option<ByronParams>,
    pub alonzo: Option<AlonzoParams>,
    pub shelley: Option<ShelleyParams>,
    pub babbage: Option<BabbageParams>,
    pub conway: Option<ConwayParams>,
}

//
// Byron protocol parameters
//

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ByronParams {
    pub block_version_data: BlockVersionData,
    pub fts_seed: Option<Vec<u8>>,
    pub protocol_consts: ProtocolConsts,
    pub start_time: u64,
}

//
// Alonzo protocol parameters
//

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AlonzoParams {
    pub lovelace_per_utxo_word: u64, // Deprecated after transition to Babbage
    pub execution_prices: ExUnitPrices,
    pub max_tx_ex_units: ExUnits,
    pub max_block_ex_units: ExUnits,
    pub max_value_size: u32,
    pub collateral_percentage: u32,
    pub max_collateral_inputs: u32,
    pub plutus_v1_cost_model: Option<CostModel>,
}

//
// Shelley protocol parameters
//

#[serde_as]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelleyProtocolParams {
    pub protocol_version: ProtocolVersion,
    pub max_tx_size: u32,
    pub max_block_body_size: u32,
    pub max_block_header_size: u32,
    pub key_deposit: u64,
    #[serde(rename = "minUTxOValue")]
    pub min_utxo_value: u64,

    #[serde(rename = "minFeeA")]
    pub minfee_a: u32,

    #[serde(rename = "minFeeB")]
    pub minfee_b: u32,
    pub pool_deposit: u64,

    /// AKA desired_number_of_stake_pools, n_opt, technical parameter k
    /// Important: *not to be mixed* with security parameter k, which is not here
    #[serde(rename = "nOpt")]
    pub stake_pool_target_num: u32,
    pub min_pool_cost: u64,

    /// AKA eMax, e_max
    #[serde(rename = "eMax")]
    pub pool_retire_max_epoch: u64,
    pub extra_entropy: Nonce,
    #[serde_as(as = "ChameleonFraction")]
    pub decentralisation_param: RationalNumber,

    /// AKA Rho, expansion_rate
    #[serde(rename = "rho")]
    #[serde_as(as = "ChameleonFraction")]
    pub monetary_expansion: RationalNumber,

    /// AKA Tau, treasury_growth_rate
    #[serde(rename = "tau")]
    #[serde_as(as = "ChameleonFraction")]
    pub treasury_cut: RationalNumber,

    /// AKA a0
    #[serde(rename = "a0")]
    #[serde_as(as = "ChameleonFraction")]
    pub pool_pledge_influence: RationalNumber,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelleyParams {
    #[serde_as(as = "ChameleonFraction")]
    pub active_slots_coeff: RationalNumber,
    pub epoch_length: u32,
    pub max_kes_evolutions: u32,
    pub max_lovelace_supply: u64,
    pub network_id: NetworkId,
    pub network_magic: u32,
    pub protocol_params: ShelleyProtocolParams,

    /// Ouroboros security parameter k: the Shardagnostic security paramaters,
    /// aka @k@. This is the maximum number of blocks the node would ever be
    /// prepared to roll back by. Clients of the node following the chain should
    /// be prepared to handle the node switching forks up to this long.
    /// (source: GenesisParameters.hs)
    pub security_param: u32,

    pub slot_length: u32,
    pub slots_per_kes_period: u32,
    pub system_start: DateTime<Utc>,
    pub update_quorum: u32,
}

//
// Babbage protocol parameters
//

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BabbageParams {
    pub coins_per_utxo_byte: u64,
    pub plutus_v2_cost_model: Option<CostModel>,
}

//
// Conway protocol parameters
//

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConwayParams {
    pub pool_voting_thresholds: PoolVotingThresholds,
    pub d_rep_voting_thresholds: DRepVotingThresholds,
    pub committee_min_size: u64,
    pub committee_max_term_length: u32,
    pub gov_action_lifetime: u32,
    pub gov_action_deposit: u64,
    pub d_rep_deposit: u64,
    pub d_rep_activity: u32,
    pub min_fee_ref_script_cost_per_byte: RationalNumber,
    pub plutus_v3_cost_model: CostModel,
    pub constitution: Constitution,
    pub committee: Committee,
}

#[derive(Serialize, PartialEq, Deserialize, Debug, Clone)]
pub struct AlonzoBabbageUpdateProposal {
    pub proposals: Vec<(GenesisKeyhash, Box<ProtocolParamUpdate>)>,
    pub enactment_epoch: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParameterChangeAction {
    pub previous_action_id: Option<GovActionId>,
    pub protocol_param_update: Box<ProtocolParamUpdate>,
    pub script_hash: Option<Vec<u8>>,
}

pub type GenesisKeyhash = Vec<u8>;

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParamUpdate {
    /// The following are the fields from Conway ProtocolParamUpdate structure
    /// AKA txFeePerByte, tx_fee_per_byte (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub minfee_a: Option<u64>,

    /// AKA txFeeFixed, tx_fee_fixed (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub minfee_b: Option<u64>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_block_body_size: Option<u64>,

    /// AKA max_tx_size (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_transaction_size: Option<u64>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_block_header_size: Option<u64>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub key_deposit: Option<Lovelace>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub pool_deposit: Option<Lovelace>,

    /// AKA poolRetireMaxEpoch, eMax (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub maximum_epoch: Option<u64>,

    /// AKA stakePoolTargetNum, nOpt (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub desired_number_of_stake_pools: Option<u64>,

    /// AKA a0 (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub pool_pledge_influence: Option<RationalNumber>,

    /// AKA rho, monetary_expansion (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub expansion_rate: Option<RationalNumber>,

    /// AKA tau, treasury_cut (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub treasury_growth_rate: Option<RationalNumber>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub min_pool_cost: Option<Lovelace>,

    /// AKA lovelacePerUTxOWord, utxoCostPerWord (Alonzo)
    /// TODO: was there any moment, when this value had different
    /// meaning? (words were recounted to bytes)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub ada_per_utxo_byte: Option<Lovelace>,

    /// AKA plutus_v1_cost_model, plutus_v2_cost_model (Shelley)
    /// plutus_v3_cost_model (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub cost_models_for_script_languages: Option<CostModels>,

    /// AKA execution_prices (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub execution_costs: Option<ExUnitPrices>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_tx_ex_units: Option<ExUnits>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_block_ex_units: Option<ExUnits>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_value_size: Option<u64>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub collateral_percentage: Option<u64>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_collateral_inputs: Option<u64>,

    /// (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub pool_voting_thresholds: Option<PoolVotingThresholds>,

    /// (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub drep_voting_thresholds: Option<DRepVotingThresholds>,

    /// (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub min_committee_size: Option<u64>,

    /// AKA committee_max_term_limit (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub committee_term_limit: Option<u64>,

    /// AKA gov_action_lifetime (Cownay)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub governance_action_validity_period: Option<u64>,

    /// AKA gov_action_deposit (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub governance_action_deposit: Option<Lovelace>,

    /// AKA d_rep_deposit (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub drep_deposit: Option<Lovelace>,

    /// AKA drep_inactivity (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub drep_inactivity_period: Option<u64>,

    /// AKA min_fee_ref_script_cost_per_byte (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub minfee_refscript_cost_per_byte: Option<RationalNumber>,

    /// The following are the fields from Alonzo-compatible ProtocolParamUpdate
    /// structure, not present in Conway.
    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub decentralisation_constant: Option<RationalNumber>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub extra_enthropy: Option<Nonce>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub protocol_version: Option<ProtocolVersion>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProtocolConsts {
    pub k: usize,
    pub protocol_magic: u32,
    pub vss_max_ttl: Option<u32>,
    pub vss_min_ttl: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockVersionData {
    pub script_version: u16,
    pub heavy_del_thd: u64,
    pub max_block_size: u64,
    pub max_header_size: u64,
    pub max_proposal_size: u64,
    pub max_tx_size: u64,
    pub mpc_thd: u64,
    pub slot_duration: u64,

    pub softfork_rule: SoftForkRule,
    pub tx_fee_policy: TxFeePolicy,

    pub unlock_stake_epoch: u64,
    pub update_implicit: u64,
    pub update_proposal_thd: u64,
    pub update_vote_thd: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SoftForkRule {
    pub init_thd: u64,
    pub min_thd: u64,
    pub thd_decrement: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TxFeePolicy {
    pub multiplier: u64,
    pub summand: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct CostModel(Vec<i64>);

impl CostModel {
    pub fn new(m: Vec<i64>) -> Self {
        CostModel(m)
    }

    pub fn as_vec(&self) -> &Vec<i64> {
        &self.0
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct CostModels {
    pub plutus_v1: Option<CostModel>,
    pub plutus_v2: Option<CostModel>,
    pub plutus_v3: Option<CostModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NetworkId {
    Testnet,
    Mainnet,
}

#[derive(Serialize, PartialEq, Deserialize, Debug, Clone)]
pub struct Constitution {
    pub anchor: Anchor,
    pub guardrail_script: Option<ScriptHash>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolVersion {
    pub minor: u64,
    pub major: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum NonceVariant {
    NeutralNonce,
    Nonce,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nonce {
    pub tag: NonceVariant,
    pub hash: Option<Vec<u8>>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct PoolVotingThresholds {
    pub motion_no_confidence: RationalNumber,
    pub committee_normal: RationalNumber,
    pub committee_no_confidence: RationalNumber,
    pub hard_fork_initiation: RationalNumber,
    pub security_voting_threshold: RationalNumber,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct DRepVotingThresholds {
    pub motion_no_confidence: RationalNumber,
    pub committee_normal: RationalNumber,
    pub committee_no_confidence: RationalNumber,
    pub update_constitution: RationalNumber,
    pub hard_fork_initiation: RationalNumber,
    pub pp_network_group: RationalNumber,
    pub pp_economic_group: RationalNumber,
    pub pp_technical_group: RationalNumber,
    pub pp_governance_group: RationalNumber,
    pub treasury_withdrawal: RationalNumber,
}
