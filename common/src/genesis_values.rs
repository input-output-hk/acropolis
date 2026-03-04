use anyhow::{bail, Result};
use blake2::{digest::consts::U32, Blake2b, Digest};
use serde::Deserialize;

use crate::{
    calculations::{
        epoch_to_first_slot_with_shelley_params, slot_to_epoch_with_shelley_params,
        slot_to_timestamp_with_params,
    },
    hash::Hash,
    protocol_params::{PraosParams, ShelleyParams},
    GenesisDelegates, MagicNumber,
};

const MAINNET_BYRON_GENESIS: &[u8] =
    include_bytes!("../../modules/genesis_bootstrapper/downloads/mainnet-byron-genesis.json");
const MAINNET_SHELLEY_GENESIS: &[u8] =
    include_bytes!("../../modules/genesis_bootstrapper/downloads/mainnet-shelley-genesis.json");
const MAINNET_SHELLEY_START_EPOCH: u64 = 208;

const PREVIEW_BYRON_GENESIS: &[u8] =
    include_bytes!("../../modules/genesis_bootstrapper/downloads/preview-byron-genesis.json");
const PREVIEW_SHELLEY_GENESIS: &[u8] =
    include_bytes!("../../modules/genesis_bootstrapper/downloads/preview-shelley-genesis.json");
const PREVIEW_SHELLEY_START_EPOCH: u64 = 0;

const SANCHONET_BYRON_GENESIS: &[u8] =
    include_bytes!("../../modules/genesis_bootstrapper/downloads/sanchonet-byron-genesis.json");
const SANCHONET_SHELLEY_GENESIS: &[u8] =
    include_bytes!("../../modules/genesis_bootstrapper/downloads/sanchonet-shelley-genesis.json");
const SANCHONET_SHELLEY_START_EPOCH: u64 = 0;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct GenesisValues {
    pub byron_timestamp: u64,
    pub shelley_epoch: u64,
    pub shelley_epoch_len: u64,
    pub shelley_genesis_hash: Hash<32>,
    pub genesis_delegs: GenesisDelegates,
    pub magic_number: MagicNumber,
}

impl GenesisValues {
    pub fn mainnet() -> Self {
        Self::for_network("mainnet").expect("embedded mainnet genesis values must be valid")
    }

    pub fn preview() -> Self {
        Self::for_network("preview").expect("embedded preview genesis values must be valid")
    }

    pub fn sanchonet() -> Self {
        Self::for_network("sanchonet").expect("embedded sanchonet genesis values must be valid")
    }

    pub fn for_network(network_name: &str) -> Result<Self> {
        let (byron, shelley, shelley_start_epoch) = network_genesis_files(network_name)?;

        let byron_cfg: ByronGenesis = serde_json::from_slice(byron)?;
        let shelley_cfg: ShelleyParams = serde_json::from_slice(shelley)?;

        Ok(Self {
            byron_timestamp: byron_cfg.start_time,
            shelley_epoch: shelley_start_epoch,
            shelley_epoch_len: shelley_cfg.epoch_length as u64,
            shelley_genesis_hash: hash_genesis_bytes(shelley),
            genesis_delegs: shelley_cfg.gen_delegs,
            magic_number: MagicNumber::new(byron_cfg.protocol_consts.protocol_magic),
        })
    }

    pub fn praos_params_for_network(network_name: &str) -> Result<PraosParams> {
        let (_, shelley, _) = network_genesis_files(network_name)?;
        let shelley_cfg: ShelleyParams = serde_json::from_slice(shelley)?;
        Ok(PraosParams::from(&shelley_cfg))
    }

    pub fn slot_to_epoch(&self, slot: u64) -> (u64, u64) {
        slot_to_epoch_with_shelley_params(slot, self.shelley_epoch, self.shelley_epoch_len)
    }
    pub fn slot_to_timestamp(&self, slot: u64) -> u64 {
        slot_to_timestamp_with_params(slot, self.byron_timestamp, self.shelley_epoch)
    }

    pub fn epoch_to_first_slot(&self, epoch: u64) -> u64 {
        epoch_to_first_slot_with_shelley_params(epoch, self.shelley_epoch, self.shelley_epoch_len)
    }
}

fn hash_genesis_bytes(raw_bytes: &[u8]) -> Hash<32> {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(raw_bytes);
    let hash: [u8; 32] = hasher.finalize().into();
    Hash::<32>::new(hash)
}

fn network_genesis_files(network_name: &str) -> Result<(&'static [u8], &'static [u8], u64)> {
    match network_name {
        "mainnet" => Ok((
            MAINNET_BYRON_GENESIS,
            MAINNET_SHELLEY_GENESIS,
            MAINNET_SHELLEY_START_EPOCH,
        )),
        "preview" => Ok((
            PREVIEW_BYRON_GENESIS,
            PREVIEW_SHELLEY_GENESIS,
            PREVIEW_SHELLEY_START_EPOCH,
        )),
        "sanchonet" | "sancho" => Ok((
            SANCHONET_BYRON_GENESIS,
            SANCHONET_SHELLEY_GENESIS,
            SANCHONET_SHELLEY_START_EPOCH,
        )),
        unsupported => bail!("Unsupported network for genesis values: {unsupported}"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ByronGenesis {
    start_time: u64,
    protocol_consts: ByronProtocolConsts,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ByronProtocolConsts {
    protocol_magic: u32,
}
