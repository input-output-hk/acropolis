use anyhow::{anyhow, Result};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AddressDeltaPublishMode {
    #[default]
    Compact,
    Extended,
}

impl FromStr for AddressDeltaPublishMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "compact" => Ok(Self::Compact),
            "extended" => Ok(Self::Extended),
            _ => Err(anyhow!(
                "Invalid address-delta-publish-mode '{s}', expected 'compact' or 'extended'"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AddressDeltaPublishMode;
    use std::str::FromStr;

    #[test]
    fn publish_mode_parser_accepts_known_values() {
        assert_eq!(
            AddressDeltaPublishMode::from_str("compact").unwrap(),
            AddressDeltaPublishMode::Compact
        );
        assert_eq!(
            AddressDeltaPublishMode::from_str("extended").unwrap(),
            AddressDeltaPublishMode::Extended
        );
    }

    #[test]
    fn publish_mode_parser_rejects_unknown_values() {
        let err = AddressDeltaPublishMode::from_str("dual").expect_err("dual is unsupported");
        assert!(err.to_string().contains("address-delta-publish-mode"));
    }
}
