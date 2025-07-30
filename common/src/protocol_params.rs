use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use crate::rational_number::{rational_number_from_f32, RationalNumber};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ChameleonFraction {
    Float(f32),
    Fraction {numerator: u64, denominator: u64}
}

impl ChameleonFraction {
    pub fn get_rational(&self) -> anyhow::Result<RationalNumber> {
        match self {
            ChameleonFraction::Fraction { numerator: n, denominator: d } =>
                Ok(RationalNumber::new(*n, *d)),
            ChameleonFraction::Float(v) => rational_number_from_f32(*v)
        }
    }

    pub fn from_rational(rational: RationalNumber) -> ChameleonFraction {
        ChameleonFraction::Fraction{
            numerator: *rational.numer(),
            denominator: *rational.denom()
        }
    }

    pub fn from_f32(f: f32) -> ChameleonFraction {
        ChameleonFraction::Float(f)
    }

    pub fn new_rational(numerator: u64, denominator: u64) -> Self {
        ChameleonFraction::Fraction {
            numerator,
            denominator,
        }
    }
}

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    pub decentralisation_param: ChameleonFraction,

    /// AKA Rho, expansion_rate
    #[serde(rename="rho")]
    pub monetary_expansion: ChameleonFraction,

    /// AKA Tau, treasury_growth_rate
    #[serde(rename="tau")]
    pub treasury_cut: ChameleonFraction,

    /// AKA a0
    #[serde(rename="a0")]
    pub pool_pledge_influence: ChameleonFraction,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NetworkId {
    Testnet,
    Mainnet,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelleyParams {
    pub active_slots_coeff: ChameleonFraction,
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
