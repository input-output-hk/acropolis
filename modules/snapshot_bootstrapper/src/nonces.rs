use acropolis_common::protocol_params::{Nonce, Nonces};
use serde::Deserialize;
use std::fs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NoncesError {
    #[error("Failed to read nonces file: {0}")]
    ReadFile(std::io::Error),

    #[error("Failed to parse nonces file: {0}")]
    Parse(serde_json::Error),

    #[error("Invalid point format: {0}")]
    InvalidPoint(String),

    #[error("Invalid hex: {0}")]
    InvalidHex(String),
}

/// Parsed nonces.json file.
#[derive(Debug, Deserialize)]
pub struct NoncesFile {
    at: String,
    active: String,
    candidate: String,
    evolving: String,
    tail: String,
}

impl NoncesFile {
    /// Load from `{dir}/nonces.json`
    pub fn load(dir: &str) -> Result<Self, NoncesError> {
        let path = format!("{dir}/nonces.json");
        let content = fs::read_to_string(&path).map_err(NoncesError::ReadFile)?;
        serde_json::from_str(&content).map_err(NoncesError::Parse)
    }

    /// Extract the hash from the `at` field (format: "slot.hash")
    pub fn at_hash(&self) -> Result<&str, NoncesError> {
        self.at
            .split_once('.')
            .map(|(_, hash)| hash)
            .ok_or_else(|| NoncesError::InvalidPoint(self.at.clone()))
    }

    /// Convert to Nonces domain type.
    ///
    /// - `epoch`: target epoch
    /// - `lab_hash`: hash of last applied block (from header)
    pub fn into_nonces(self, epoch: u64, lab_hash: [u8; 32]) -> Result<Nonces, NoncesError> {
        Ok(Nonces {
            epoch,
            active: parse_nonce(&self.active)?,
            evolving: parse_nonce(&self.evolving)?,
            candidate: parse_nonce(&self.candidate)?,
            lab: Nonce::from(lab_hash),
            prev_lab: parse_nonce(&self.tail)?,
        })
    }
}

fn parse_nonce(hex_str: &str) -> Result<Nonce, NoncesError> {
    let bytes = hex::decode(hex_str).map_err(|_| NoncesError::InvalidHex(hex_str.to_string()))?;
    let hash: [u8; 32] =
        bytes.try_into().map_err(|_| NoncesError::InvalidHex(hex_str.to_string()))?;
    Ok(Nonce::from(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nonce() {
        let nonce = parse_nonce("0b9e320e63bf995b81287ce7a624b6735d98b083cc1a0e2ae8b08b680c79c983")
            .unwrap();
        assert!(nonce.hash.is_some());

        assert!(parse_nonce("invalid").is_err());
        assert!(parse_nonce("abcd").is_err()); // too short
    }
}
