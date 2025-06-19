use anyhow::{anyhow, Error, Result};
use gcd::Gcd;
use fraction::Fraction;
use std::cmp::Ordering;

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct RationalNumber {
    pub numerator: u64,
    pub denominator: u64,
}

impl RationalNumber {
    pub fn new(numerator: u64, denominator: u64) -> anyhow::Result<Self> {
        if denominator == 0 {
            Err(anyhow!(
                "{numerator}/{denominator}: denominator cannot be zero"
            ))
        } else {
            Ok(Self {
                numerator,
                denominator,
            })
        }
    }

    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    pub const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    pub fn proportion_of(&self, value: u64) -> anyhow::Result<Self> {
        let gcd = Gcd::gcd(self.denominator, value);
        let value_gcd: u64 = value / gcd;
        let new_numerator = value_gcd
            .checked_mul(self.numerator)
            .ok_or_else(|| anyhow!("u64 overflow in {} * {}", value_gcd, self.numerator))?;
        Self::new(new_numerator, self.denominator / gcd)
    }

    pub fn round_up(&self) -> u64 {
        let quot = self.numerator / self.denominator;
        let rem = self.numerator % self.denominator;

        if rem != 0 {
            quot + 1
        } else {
            quot
        }
    }
}

impl From<u64> for RationalNumber {
    fn from(value: u64) -> Self {
        Self {
            numerator: value,
            denominator: 1,
        }
    }
}

impl TryFrom<f32> for RationalNumber {
    type Error = Error;
    fn try_from(value: f32) -> Result<RationalNumber> {
        if value.is_sign_negative() {
            return Err(anyhow!("Value {} must be greater than 0", value));
        }
        let fract = Fraction::from(value);
        Ok(RationalNumber {
            numerator: *fract
                .numer()
                .ok_or_else(|| anyhow!("Cannot get numerator for {}", value))?,
            denominator: *fract
                .denom()
                .ok_or_else(|| anyhow!("Cannot get denominator for {}", value))?,
        })
    }
}

impl PartialOrd for RationalNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(u64::cmp(
            &(self.numerator * other.denominator),
            &(self.denominator * other.numerator),
        ))
    }
}

impl Ord for RationalNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        u64::cmp(
            &(self.numerator * other.denominator),
            &(self.denominator * other.numerator),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::rational_number::RationalNumber;

    #[test]
    fn test_fractions() -> Result<(), anyhow::Error> {
        assert_eq!(
            RationalNumber::try_from(0.51)?,
            RationalNumber::new(51, 100)?
        );
        assert_eq!(RationalNumber::try_from(0.67)?, RationalNumber::new(67,100)?);
        assert_eq!(RationalNumber::try_from(0.6)?, RationalNumber::new(3,5)?);
        assert_eq!(RationalNumber::try_from(0.75)?, RationalNumber::new(3,4)?);
        assert_eq!(RationalNumber::try_from(0.5)?, RationalNumber::new(1,2)?);
        Ok(())
    }
}
