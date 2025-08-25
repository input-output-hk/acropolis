use anyhow::{anyhow, Result};
use bigdecimal::BigDecimal;
use num_traits::ToPrimitive;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{DeserializeAs, SerializeAs};
use std::str::FromStr;

pub type RationalNumber = num_rational::Ratio<u64>;

pub fn rational_number_from_f32(f: f32) -> Result<RationalNumber> {
    RationalNumber::approximate_float_unsigned(f)
        .ok_or_else(|| anyhow!("Cannot convert {f} to Rational"))
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
#[serde(untagged)]
pub enum ChameleonFraction {
    Float(f32),
    Fraction { numerator: u64, denominator: u64 },
}

impl ChameleonFraction {
    const MAX_ROUND_DECIMAL: u64 = 10_000_000_000_000_000_000u64;

    fn div_dec_00(d: u64) -> bool {
        Self::MAX_ROUND_DECIMAL % d == 0
    }

    pub fn get_rational(&self) -> anyhow::Result<RationalNumber> {
        match self {
            ChameleonFraction::Fraction {
                numerator: n,
                denominator: d,
            } => Ok(RationalNumber::new(*n, *d)),
            ChameleonFraction::Float(v) => rational_number_from_f32(*v),
        }
    }

    pub fn get_big_decimal(&self) -> anyhow::Result<BigDecimal> {
        match self {
            ChameleonFraction::Fraction { denominator: d, .. } if !Self::div_dec_00(*d) => {
                anyhow::bail!("Denominator {d} must divide {}", Self::MAX_ROUND_DECIMAL)
            }
            _ => self.get_approx_big_decimal(),
        }
    }

    pub fn get_approx_big_decimal(&self) -> anyhow::Result<BigDecimal> {
        match self {
            ChameleonFraction::Fraction {
                numerator: n,
                denominator: d,
            } => Ok(BigDecimal::from(n) / BigDecimal::from(d)),
            ChameleonFraction::Float(v) => Ok(BigDecimal::from_str(&v.to_string())?),
        }
    }

    pub fn from_rational(rational: RationalNumber) -> ChameleonFraction {
        ChameleonFraction::Fraction {
            numerator: *rational.numer(),
            denominator: *rational.denom(),
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
            ChameleonFraction::Fraction {
                numerator: n,
                denominator: d,
            } => (*d > 0 && n % d == 0).then(|| (n / d).try_into().ok()).flatten(),
        }
    }

    fn to_u64(&self) -> Option<u64> {
        match self {
            ChameleonFraction::Float(f) => f.to_u64(),
            ChameleonFraction::Fraction {
                numerator: n,
                denominator: d,
            } => (*d > 0 && n % d == 0).then_some(n / d),
        }
    }

    fn to_f64(&self) -> Option<f64> {
        match self {
            ChameleonFraction::Float(v) => Some(*v as f64),
            ChameleonFraction::Fraction {
                numerator: n,
                denominator: d,
            } => RationalNumber::new(*n, *d).to_f64(),
        }
    }
}

impl SerializeAs<RationalNumber> for ChameleonFraction {
    fn serialize_as<S>(src: &RationalNumber, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let ch = ChameleonFraction::from_rational(*src);
        ch.serialize(serializer)
    }
}

impl<'de> DeserializeAs<'de, RationalNumber> for ChameleonFraction {
    fn deserialize_as<D>(deserializer: D) -> std::result::Result<RationalNumber, D::Error>
    where
        D: Deserializer<'de>,
    {
        match ChameleonFraction::deserialize(deserializer) {
            Ok(v) => match v.get_rational() {
                Ok(r) => Ok(r),
                Err(ce) => Err(D::Error::custom(ce)),
            },
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    //use crate::rational_number::RationalNumber;
    //use crate::rational_number::rational_number_from_f32;

    #[test]
    fn test_fractions() -> Result<(), anyhow::Error> {
        assert_eq!(
            rational_number_from_f32(0.51)?,
            RationalNumber::new(51, 100)
        );
        assert_eq!(
            rational_number_from_f32(0.67)?,
            RationalNumber::new(67, 100)
        );
        assert_eq!(rational_number_from_f32(0.6)?, RationalNumber::new(3, 5));
        assert_eq!(rational_number_from_f32(0.75)?, RationalNumber::new(3, 4));
        assert_eq!(rational_number_from_f32(0.5)?, RationalNumber::new(1, 2));
        Ok(())
    }

    #[test]
    fn test_chameleon_serialization() -> Result<()> {
        for n in 0..=1000 {
            let ch = [
                &ChameleonFraction::Float(f32::from_str(&format!("0.{:03}", n))?),
                &ChameleonFraction::Fraction {
                    numerator: n,
                    denominator: 1000,
                },
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
                ChameleonFraction::Fraction {
                    numerator: n,
                    denominator: 1000
                }
                .get_big_decimal()?
                    * 1000,
                BigDecimal::from(n)
            );
        }

        let mut twos = 1;
        for _t in 0..=19 {
            let mut fives = 1;
            for _f in 0..=19 {
                assert_eq!(
                    ChameleonFraction::Fraction {
                        numerator: 777,
                        denominator: twos * fives
                    }
                    .get_big_decimal()?
                        * BigDecimal::from(twos * fives),
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
        let fraction777 = ChameleonFraction::Fraction {
            numerator: 3,
            denominator: 777,
        };
        if let Ok(v) = fraction777.get_big_decimal() {
            anyhow::bail!(
                "{fraction777:?} cannot be represented in big decimal, although we have {v:?}"
            );
        }

        assert_ne!(
            ChameleonFraction::Fraction {
                numerator: 3,
                denominator: 777
            }
            .get_approx_big_decimal()?
                * 777,
            BigDecimal::from(3)
        );

        Ok(())
    }
}
