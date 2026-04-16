use anyhow::{anyhow, Result};
use config::Config;
use std::{path::Path, sync::Arc};
use tracing::info;

use acropolis_common::{
    configuration::{conf_enum, get_bool_flag, get_string_flag, StartupMode},
    NetworkId,
};

use crate::utils::CacheMode;

const DEFAULT_STAKE_ADDRESS_DELTA_TOPIC: (&str, &str) =
    ("publishing-stake-delta-topic", "cardano.stake.deltas");
const DEFAULT_VALIDATION_TOPIC: (&str, &str) = (
    "publishing-validation-topic",
    "cardano.validation.stake.filter",
);
/// Directory to put cached shelley address pointers into. Depending on the address
/// cache mode, these cached pointers can be used instead of tracking current pointer
/// values in blockchain (which can be quite resource-consuming).
const DEFAULT_CACHE_DIR: (&str, &str) = ("cache-dir", "cache");

/// Cache mode: use built-in; always build cache; always read cache (error if missing);
/// build if missing, read otherwise.
const DEFAULT_CACHE_MODE: (&str, CacheMode) = ("cache-mode", CacheMode::Predefined);

/// Cache remembers all stake addresses that could potentially be referenced by pointers. However
/// only a few addressed are actually referenced by pointers in real blockchain.
/// `true` means that all possible addresses should be written to disk (as potential pointers).
/// `false` means that only addresses used in actual pointers should be written to disk.
const DEFAULT_WRITE_FULL_CACHE: (&str, bool) = ("write-full-cache", false);

#[derive(Clone, Debug, Default)]
pub struct StakeDeltaFilterParams {
    pub stake_address_delta_topic: String,
    pub validation_topic: String,

    pub cache_dir: String,
    pub cache_mode: CacheMode,
    pub write_full_cache: bool,
    pub is_snapshot_mode: bool,
}

impl StakeDeltaFilterParams {
    pub fn get_cache_file_name(&self, modifier: &str, network: &NetworkId) -> Result<String> {
        let path = Path::new(&self.cache_dir);
        let full = path.join(format!("{:?}{}", network, modifier).to_lowercase());
        let str =
            full.to_str().ok_or_else(|| anyhow!("Cannot produce cache file name".to_string()))?;
        Ok(str.to_string())
    }

    pub fn init(cfg: Arc<Config>) -> Result<Arc<Self>> {
        let params = Self {
            stake_address_delta_topic: get_string_flag(&cfg, DEFAULT_STAKE_ADDRESS_DELTA_TOPIC),
            validation_topic: get_string_flag(&cfg, DEFAULT_VALIDATION_TOPIC),
            cache_dir: get_string_flag(&cfg, DEFAULT_CACHE_DIR),
            cache_mode: conf_enum::<CacheMode>(&cfg, DEFAULT_CACHE_MODE)?,
            write_full_cache: get_bool_flag(&cfg, DEFAULT_WRITE_FULL_CACHE),
            is_snapshot_mode: StartupMode::from_config(cfg.as_ref()).is_snapshot(),
        };

        info!("Cache mode {:?}", params.cache_mode);
        if params.cache_mode == CacheMode::Read {
            if !Path::new(&params.cache_dir).try_exists()? {
                return Err(anyhow!(
                    "Pointer cache directory '{}' does not exist.",
                    params.cache_dir
                ));
            }
            info!("Reading (writing) caches from (to) {}", params.cache_dir);
        } else if params.cache_mode != CacheMode::Predefined {
            std::fs::create_dir_all(&params.cache_dir)?;
        }

        Ok(Arc::new(params))
    }
}
