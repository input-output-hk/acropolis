use crate::block::{BlockContext, BlockContextError};
use crate::configuration::{BootstrapConfig, ConfigError, Snapshot};
use crate::nonces::{NonceContext, NonceContextError};
use crate::publisher::EpochContext;
use acropolis_common::{
    genesis_values::GenesisValues, protocol_params::Nonces, BlockInfo, BlockIntent, BlockStatus,
    Point,
};
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
    Block(#[from] BlockContextError),

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

        // Load block
        let block_ctx = BlockContext::load(&network_dir, &snapshot.point)?;
        let hash = block_ctx
            .point
            .hash()
            .unwrap_or_else(|| panic!("Origin point has no hash: {:?}", block_ctx.point));
        let slot = block_ctx.point.slot();

        // Build nonce
        let nonces = nonces_file.into_nonces(target_epoch, *hash);

        // Build block info
        let (_, epoch_slot) = genesis.slot_to_epoch(slot);
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
            slot,
            number: block_ctx.block_number,
            hash: *hash,
            epoch: target_epoch,
            epoch_slot,
            new_epoch: false,
            timestamp: genesis.slot_to_timestamp(slot),
            tip_slot: None,
            era: block_ctx.era,
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
            self.block_info.era,
            &self.genesis,
        )
    }
}

fn genesis_for_network(_network: &str) -> GenesisValues {
    // TODO: Add preprod/preview support
    GenesisValues::mainnet()
}
