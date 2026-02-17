use acropolis_common::{DRepCredential, StakeAddress};
use anyhow::Result;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DRepDelegationContextError {
    #[error("Failed to read {0}: {1}")]
    ReadFile(PathBuf, std::io::Error),

    #[error("Failed to parse {0}: {1}")]
    Parse(PathBuf, serde_json::Error),

    #[error("Invalid DRep credential: {0}")]
    InvalidDRep(String),

    #[error("Invalid stake address: {0}")]
    InvalidStake(String),
}

#[derive(Debug, serde::Deserialize)]
struct RawDRepDelegations(BTreeMap<String, Vec<String>>);

pub struct DRepDelegationContext {
    pub delegations: Vec<(DRepCredential, Vec<StakeAddress>)>,
}

impl DRepDelegationContext {
    pub fn path(network_dir: &Path) -> PathBuf {
        network_dir.join("drep_delegations.json")
    }

    pub fn load(network_dir: &Path) -> Result<Self, DRepDelegationContextError> {
        let path = Self::path(network_dir);

        let content = fs::read_to_string(&path)
            .map_err(|e| DRepDelegationContextError::ReadFile(path.clone(), e))?;

        let raw: RawDRepDelegations = serde_json::from_str(&content)
            .map_err(|e| DRepDelegationContextError::Parse(path.clone(), e))?;

        let mut delegations = Vec::with_capacity(raw.0.len());

        for (drep_bech32, stake_bech32s) in raw.0 {
            let drep = DRepCredential::from_drep_bech32(&drep_bech32)
                .map_err(|_| DRepDelegationContextError::InvalidDRep(drep_bech32.clone()))?;

            let mut stakes = Vec::with_capacity(stake_bech32s.len());
            for s in stake_bech32s {
                let stake = StakeAddress::from_string(&s)
                    .map_err(|_| DRepDelegationContextError::InvalidStake(s.clone()))?;
                stakes.push(stake);
            }

            delegations.push((drep, stakes));
        }

        Ok(Self { delegations })
    }
}
