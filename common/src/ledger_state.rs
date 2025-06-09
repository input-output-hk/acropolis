use crate::{KeyHash, PoolRegistration};
use anyhow::{bail, Context, Result};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct LedgerState {
    pub spo_state: SPOState,
}

pub struct UTxOState {}

pub struct StakeDistributionState {}

pub struct AccountState {}

pub struct ParametersState {}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, minicbor::Decode, minicbor::Encode, Default,
)]
pub struct SPOState {
    #[n(0)]
    pub pools: BTreeMap<KeyHash, PoolRegistration>,
    #[n(1)]
    pub retiring: BTreeMap<KeyHash, u64>,
}

pub struct DRepState {}

pub struct ProposalState {}

pub struct VotingState {}

impl LedgerState {
    pub fn from_directory(directory_path: impl AsRef<Path>) -> Result<Self> {
        let directory_path = directory_path.as_ref();
        if !directory_path.exists() {
            bail!("directory does not exist: {:?}", directory_path);
        }

        if !directory_path.is_dir() {
            bail!("path is not a directory: {:?}", directory_path);
        }

        let mut ledger_state = Self::default();
        ledger_state
            .load_from_directory(directory_path)
            .with_context(|| {
                format!(
                    "Failed to load ledger state from directory: {:?}",
                    directory_path
                )
            })?;

        Ok(ledger_state)
    }

    fn load_from_directory(&mut self, directory_path: impl AsRef<Path>) -> Result<()> {
        let directory_path = directory_path.as_ref();
        let entries = fs::read_dir(directory_path)
            .with_context(|| format!("failed to read directory: {:?}", directory_path))?;

        for entry in entries {
            let entry = entry.with_context(|| "failed to read directory entry")?;
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |ext| ext == "cbor") {
                self.load_cbor_file(&path)
                    .with_context(|| format!("failed to load CBOR file: {:?}", path))?;
            }
        }

        Ok(())
    }

    fn load_cbor_file(&mut self, file_path: impl AsRef<Path>) -> Result<()> {
        let file_path = file_path.as_ref();
        let filename = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .with_context(|| format!("invalid filename: {:?}", file_path))?;

        let bytes =
            fs::read(file_path).with_context(|| format!("failed to read file: {:?}", file_path))?;

        match filename {
            "pools" => {
                self.spo_state = minicbor::decode(&bytes)
                    .with_context(|| format!("failed to decode SPO state from: {:?}", file_path))?;
            }
            _ => {
                // ignore unknown cbor files
            }
        }

        Ok(())
    }
}
