use crate::configuration::{BootstrapConfig, ConfigError, Snapshot};
use crate::header::{HeaderContext, HeaderContextError};
use crate::nonces::{NonceContext, NonceContextError};
use crate::publisher::EpochContext;
use acropolis_common::genesis_values::GenesisValues;
use acropolis_common::protocol_params::Nonces;
use acropolis_common::{BlockInfo, BlockStatus, Era, Point};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BootstrapContextError {
    #[error("Origin point has no hash")]
    OriginPoint,

    #[error("Nonces point mismatch: nonces at {nonces_point}, snapshot at {snapshot_point}")]
    NoncesPointMismatch {
        nonces_point: Point,
        snapshot_point: Point,
    },

    #[error(transparent)]
    Header(#[from] HeaderContextError),

    #[error(transparent)]
    Nonces(#[from] NonceContextError),

    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// Everything needed to bootstrap from a snapshot.
#[derive(Debug)]
pub struct BootstrapContext {
    pub genesis: GenesisValues,
    pub snapshot: Snapshot,
    pub nonces: Nonces,
    pub block_info: BlockInfo,
    network_dir: PathBuf,
}

impl BootstrapContext {
    /// Load all bootstrap data from the network directory.
    pub fn load(cfg: &BootstrapConfig) -> Result<Self, BootstrapContextError> {
        let target_epoch = cfg.epoch;
        let snapshot = cfg.snapshot()?;
        let network_dir = cfg.network_dir();
        let genesis = genesis_for_network(&cfg.network);

        let nonces_file = NonceContext::load(&network_dir)?;

        // Validate nonces match snapshot point
        if nonces_file.at != snapshot.point {
            return Err(BootstrapContextError::NoncesPointMismatch {
                nonces_point: nonces_file.at.clone(),
                snapshot_point: snapshot.point.clone(),
            });
        }

        // Load header
        let header = HeaderContext::load(&network_dir, &snapshot.point)?;
        let hash = header
            .point
            .hash()
            .unwrap_or_else(|| panic!("Origin point has no hash: {:?}", header.point));
        let slot = header.point.slot();

        // Build nonce
        let nonces = nonces_file.into_nonces(target_epoch, *hash);

        // Build block info
        let (_, epoch_slot) = genesis.slot_to_epoch(slot);
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            slot,
            number: header.block_number,
            hash: *hash,
            epoch: target_epoch,
            epoch_slot,
            new_epoch: true,
            timestamp: genesis.slot_to_timestamp(slot),
            era: Era::Conway, // TODO: Make dynamic with era history
        };

        Ok(Self {
            genesis,
            snapshot,
            nonces,
            block_info,
            network_dir,
        })
    }

    /// Path to the snapshot cbor file.
    pub fn snapshot_path(&self) -> PathBuf {
        self.snapshot.cbor_path(&self.network_dir)
    }

    /// Network directory path.
    pub fn network_dir(&self) -> &Path {
        &self.network_dir
    }

    /// Create the bootstrap context for the publisher.
    pub fn context(&self) -> EpochContext {
        EpochContext::new(
            self.nonces.clone(),
            self.block_info.slot,
            self.block_info.number,
            self.block_info.epoch,
            &self.genesis,
        )
    }
}

fn genesis_for_network(_network: &str) -> GenesisValues {
    // TODO: Add preprod/preview support
    GenesisValues::mainnet()
}
