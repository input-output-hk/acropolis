use chrono::{DateTime, Utc};
use serde_with::serde_as;
use crate::rational_number::{RationalNumber, ChameleonFraction};

//
// Shelley protocol parameters
//

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

#[serde_as]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelleyProtocolParams {
    pub protocol_version: ProtocolVersion,
    pub max_tx_size: u32,
    pub max_block_body_size: u32,
    pub max_block_header_size: u32,
    pub key_deposit: u64,
    #[serde(rename="minUTxOValue")]
    pub min_utxo_value: u64,

    #[serde(rename="minFeeA")]
    pub minfee_a: u32,

    #[serde(rename="minFeeB")]
    pub minfee_b: u32,
    pub pool_deposit: u64,

    /// AKA desired_number_of_stake_pools, n_opt, k parameter
    #[serde(rename="nOpt")]
    pub stake_pool_target_num: u32,
    pub min_pool_cost: u64,

    /// AKA eMax, e_max
    #[serde(rename="eMax")]
    pub pool_retire_max_epoch: u64,
    pub extra_entropy: Nonce,
    #[serde_as(as = "ChameleonFraction")]
    pub decentralisation_param: RationalNumber,

    /// AKA Rho, expansion_rate
    #[serde(rename="rho")]
    #[serde_as(as = "ChameleonFraction")]
    pub monetary_expansion: RationalNumber,

    /// AKA Tau, treasury_growth_rate
    #[serde(rename="tau")]
    #[serde_as(as = "ChameleonFraction")]
    pub treasury_cut: RationalNumber,

    /// AKA a0
    #[serde(rename="a0")]
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
    pub security_param: u32,
    pub slot_length: u32,
    pub slots_per_kes_period: u32,
    pub system_start: DateTime<Utc>,
    pub update_quorum: u32,
}
