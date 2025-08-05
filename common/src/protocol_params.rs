use chrono::{DateTime, Utc};
use num_traits::ToPrimitive;
use bigdecimal::BigDecimal;
use serde::{Serialize, Deserialize};
use std::str::FromStr;
use crate::rational_number::{rational_number_from_f32, RationalNumber};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
#[serde(untagged)]
pub enum ChameleonFraction {
    Float(f32),
    Fraction {numerator: u64, denominator: u64}
}


impl ChameleonFraction {
    const MAX_ROUND_DECIMAL: u64 = 10_000_000_000_000_000_000u64;

    fn div_dec_00(d: u64) -> bool {
        Self::MAX_ROUND_DECIMAL % d == 0
    }

    pub fn get_rational(&self) -> anyhow::Result<RationalNumber> {
        match self {
            ChameleonFraction::Fraction { numerator: n, denominator: d } =>
                Ok(RationalNumber::new(*n, *d)),
            ChameleonFraction::Float(v) => rational_number_from_f32(*v)
        }
    }

    pub fn get_big_decimal(&self) -> anyhow::Result<BigDecimal> {
        match self {
            ChameleonFraction::Fraction { denominator: d, .. } if !Self::div_dec_00(*d) =>
                anyhow::bail!("Denominator {d} must divide {}", Self::MAX_ROUND_DECIMAL),
            _ => self.get_approx_big_decimal()
        }
    }

    pub fn get_approx_big_decimal(&self) -> anyhow::Result<BigDecimal> {
        match self {
            ChameleonFraction::Fraction { numerator: n, denominator: d } =>
                Ok(BigDecimal::from(n) / BigDecimal::from(d)),
            ChameleonFraction::Float(v) => Ok(BigDecimal::from_str(&v.to_string())?)
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

impl ToPrimitive for ChameleonFraction {
    fn to_i64(&self) -> Option<i64> {
        match self {
            ChameleonFraction::Float(f) => f.to_i64(),
            ChameleonFraction::Fraction { numerator: n, denominator: d } =>
                (*d > 0 && n % d == 0).then(|| (n / d).try_into().ok()).flatten()
        }
    }

    fn to_u64(&self) -> Option<u64> {
        match self {
            ChameleonFraction::Float(f) => f.to_u64(),
            ChameleonFraction::Fraction { numerator: n, denominator: d } =>
                (*d > 0 && n % d == 0).then_some(n / d)
        }
    }

    fn to_f64(&self) -> Option<f64> {
        match self {
            ChameleonFraction::Float(v) => Some(*v as f64),
            ChameleonFraction::Fraction{numerator: n, denominator: d} =>
                RationalNumber::new(*n, *d).to_f64()
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NetworkId {
    Testnet,
    Mainnet,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use super::*;

    #[test]
    fn test_chameleon_serialization() -> Result<()> {
        for n in 0..=1000 {
            let ch = [
                &ChameleonFraction::Float(f32::from_str(&format!("0.{:03}", n))?),
                &ChameleonFraction::Fraction{numerator: n, denominator: 1000}
            ];

            for elem in ch {
                let elem_str = serde_json::to_string(elem).unwrap();
                let elem_back = serde_json::from_str::<ChameleonFraction>(&elem_str).unwrap();
                println!("{elem:?} => '{elem_str}'");
                assert_eq!(elem, &elem_back);
            }
        }
        Ok(())
    }

    #[test]
    fn test_big_decimal() -> Result<(), anyhow::Error> {
        for n in 0..=1000 {
            assert_eq!(
                ChameleonFraction::Fraction{numerator: n, denominator: 1000}
                    .get_big_decimal()?*1000,
                BigDecimal::from(n)
            );
        }

        let mut twos = 1;
        for _t in 0..=19 {
            let mut fives = 1;
            for _f in 0..=19 {
                assert_eq!(
                    ChameleonFraction::Fraction{numerator: 777, denominator: twos*fives}
                        .get_big_decimal()?*BigDecimal::from(twos*fives),
                    BigDecimal::from(777)
                );
                fives *= 5;
            }
            twos *= 2;
        }

        Ok(())
    }

    // ChameleonFraction does not work for non 10^n denomniators
    #[test]
    fn test_non_round_denominator() -> Result<(), anyhow::Error> {
        let fraction777 = ChameleonFraction::Fraction{numerator: 3, denominator: 777};
        if let Ok(v) = fraction777.get_big_decimal() {
            anyhow::bail!(
                "{fraction777:?} cannot be represented in big decimal, although we have {v:?}"
            );
        }

        assert_ne!(
            ChameleonFraction::Fraction{numerator: 3, denominator: 777}
                .get_approx_big_decimal()?*777,
            BigDecimal::from(3)
        );

        Ok(())
    }
}
