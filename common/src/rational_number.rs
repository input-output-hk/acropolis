use anyhow::{anyhow, Result};

pub type RationalNumber = num_rational::Ratio<u64>;

pub fn rational_number_from_f32(f: f32) -> Result<RationalNumber> {
    RationalNumber::approximate_float_unsigned(f)
        .ok_or_else(|| anyhow!("Cannot convert {f} to Rational"))
}

#[cfg(test)]
mod tests {
    use crate::rational_number::rational_number_from_f32;
    use crate::rational_number::RationalNumber;

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
}
