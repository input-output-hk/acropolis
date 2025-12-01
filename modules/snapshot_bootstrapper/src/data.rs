use crate::configuration::{BootstrapConfig, Snapshot};
use crate::header::{Header, HeaderError};
use crate::nonces::{NoncesError, NoncesFile};
use crate::publisher::BootstrapContext;
use acropolis_common::genesis_values::GenesisValues;
use acropolis_common::protocol_params::Nonces;
use acropolis_common::{BlockHash, BlockInfo, BlockStatus, Era};
use serde::Deserialize;
use std::fs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BootstrapDataError {
    #[error("Failed to read {0}: {1}")]
    ReadFile(String, std::io::Error),

    #[error("Failed to parse {0}: {1}")]
    ParseJson(String, serde_json::Error),

    #[error("Snapshot not found for epoch {0}")]
    SnapshotNotFound(u64),

    #[error("Header not found for hash: {0}")]
    HeaderNotFound(String),

    #[error("Invalid block hash: {0}")]
    InvalidBlockHash(String),

    #[error(transparent)]
    Header(#[from] HeaderError),

    #[error(transparent)]
    Nonces(#[from] NoncesError),
}

/// Everything needed to bootstrap from a snapshot.
#[derive(Debug)]
pub struct BootstrapData {
    pub genesis: GenesisValues,
    pub snapshot: Snapshot,
    pub nonces: Nonces,
    pub block_info: BlockInfo,
    network_dir: String,
}

impl BootstrapData {
    /// Load all bootstrap data from the network directory.
    pub fn load(config: &BootstrapConfig) -> Result<Self, BootstrapDataError> {
        let dir = config.network_dir();
        let genesis = genesis_for_network(&config.network);

        // config.json -> target epoch
        let target_epoch = read_json::<ConfigFile>(&format!("{dir}/config.json"))?.snapshot;

        // snapshots.json -> find snapshot for target epoch
        let snapshot = read_json::<Vec<Snapshot>>(&format!("{dir}/snapshots.json"))?
            .into_iter()
            .find(|s| s.epoch == target_epoch)
            .ok_or(BootstrapDataError::SnapshotNotFound(target_epoch))?;

        // nonces.json -> get at_hash to find header
        let nonces_file = NoncesFile::load(&dir)?;
        let nonces_hash = nonces_file.at_hash()?;

        // headers.json -> find header point matching nonces
        let header_point = read_json::<Vec<String>>(&format!("{dir}/headers.json"))?
            .into_iter()
            .find(|p| p.ends_with(nonces_hash))
            .ok_or_else(|| BootstrapDataError::HeaderNotFound(nonces_hash.to_string()))?;

        // Load header
        let header = Header::load(&dir, &header_point)?;

        // Build nonces with header hash as lab
        let nonces = nonces_file.into_nonces(target_epoch, header.hash)?;

        // Build block info
        let hash = BlockHash::try_from(header.hash.to_vec())
            .map_err(|e| BootstrapDataError::InvalidBlockHash(format!("{:?}", e)))?;

        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            slot: header.slot,
            number: header.block_number()?,
            hash,
            epoch: target_epoch,
            epoch_slot: genesis.epoch_to_first_slot(target_epoch),
            new_epoch: true,
            timestamp: genesis.slot_to_timestamp(header.slot),
            era: Era::Conway,
        };

        Ok(Self {
            genesis,
            snapshot,
            nonces,
            block_info,
            network_dir: dir,
        })
    }

    /// Path to the snapshot file.
    pub fn snapshot_path(&self) -> String {
        format!("{}/{}.cbor", self.network_dir, self.snapshot.point)
    }

    /// Network directory path.
    pub fn network_dir(&self) -> &str {
        &self.network_dir
    }

    /// Create the bootstrap context for the publisher.
    pub fn context(&self) -> BootstrapContext {
        BootstrapContext::new(
            self.nonces.clone(),
            self.block_info.slot,
            self.block_info.number,
            self.block_info.epoch,
            &self.genesis,
        )
    }
}

fn genesis_for_network(network: &str) -> GenesisValues {
    match network {
        _ => GenesisValues::mainnet(),
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T, BootstrapDataError> {
    let content =
        fs::read_to_string(path).map_err(|e| BootstrapDataError::ReadFile(path.to_string(), e))?;
    serde_json::from_str(&content).map_err(|e| BootstrapDataError::ParseJson(path.to_string(), e))
}

/// Internal: config.json structure
#[derive(Deserialize)]
struct ConfigFile {
    snapshot: u64,
}
