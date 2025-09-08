use crate::{
    rational_number::{ChameleonFraction, RationalNumber},
    BlockVersionData, Committee, Constitution, CostModel, DRepVotingThresholds, ExUnitPrices,
    ExUnits, PoolVotingThresholds, ProtocolConsts,
};
use chrono::{DateTime, Utc};
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

    /// AKA desired_number_of_stake_pools, optimal_pool_count, n_opt, technical parameter k
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NetworkId {
    Testnet,
    Mainnet,
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
