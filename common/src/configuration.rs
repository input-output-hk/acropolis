use config::Config;
use serde::Deserialize;
use std::fmt::{Display, Formatter, Result};

pub const CONFIG_KEY_STARTUP_MODE: &str = "startup.startup-mode";
pub const CONFIG_KEY_SYNC_MODE: &str = "startup.sync-mode";
pub const CONFIG_KEY_BLOCK_FLOW_MODE: &str = "startup.block-flow-mode";

pub fn get_bool_flag(config: &Config, key: (&str, bool)) -> bool {
    config.get_bool(key.0).unwrap_or(key.1)
}

pub fn get_string_flag(config: &Config, key: (&str, &str)) -> String {
    config.get_string(key.0).unwrap_or_else(|_| key.1.to_string())
}

pub fn get_u64_flag(config: &Config, key: (&str, u64)) -> u64 {
    config.get_int(key.0).ok().and_then(|v| u64::try_from(v).ok()).unwrap_or(key.1)
}

pub fn conf_enum<'a, T: Deserialize<'a>>(config: &Config, keydef: (&str, T)) -> anyhow::Result<T> {
    if config.get_string(keydef.0).is_ok() {
        config
            .get::<T>(keydef.0)
            .map_err(|e| anyhow::anyhow!("cannot parse {} value: {e}", keydef.0))
    } else {
        Ok(keydef.1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    Mithril,
    Upstream,
}

impl SyncMode {
    pub fn from_config(config: &Config) -> Self {
        config.get::<SyncMode>(CONFIG_KEY_SYNC_MODE).unwrap_or(SyncMode::Mithril)
    }

    pub fn is_mithril(&self) -> bool {
        matches!(self, SyncMode::Mithril)
    }

    pub fn is_upstream(&self) -> bool {
        matches!(self, SyncMode::Upstream)
    }
}

impl Display for SyncMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            SyncMode::Mithril => write!(f, "mithril"),
            SyncMode::Upstream => write!(f, "upstream"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupMode {
    Genesis,
    Snapshot,
}

impl StartupMode {
    pub fn from_config(config: &Config) -> Self {
        config.get::<StartupMode>(CONFIG_KEY_STARTUP_MODE).unwrap_or(StartupMode::Genesis)
    }

    pub fn is_genesis(&self) -> bool {
        matches!(self, StartupMode::Genesis)
    }

    pub fn is_snapshot(&self) -> bool {
        matches!(self, StartupMode::Snapshot)
    }
}

impl Display for StartupMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            StartupMode::Genesis => write!(f, "genesis"),
            StartupMode::Snapshot => write!(f, "snapshot"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockFlowMode {
    /// Direct: PNI auto-fetches blocks, consensus is pass-through.
    Direct,
    /// Consensus: PNI publishes offers, consensus drives fetching via wants.
    Consensus,
}

impl BlockFlowMode {
    pub fn from_config(config: &Config) -> Self {
        config.get::<BlockFlowMode>(CONFIG_KEY_BLOCK_FLOW_MODE).unwrap_or(BlockFlowMode::Direct)
    }

    pub fn is_consensus(&self) -> bool {
        matches!(self, BlockFlowMode::Consensus)
    }
}

impl Display for BlockFlowMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            BlockFlowMode::Direct => write!(f, "direct"),
            BlockFlowMode::Consensus => write!(f, "consensus"),
        }
    }
}
