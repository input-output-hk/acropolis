use config::Config;
use serde::Deserialize;
use std::fmt::{Display, Formatter, Result};

pub const CONFIG_KEY_STARTUP_MODE: &str = "startup.startup-mode";
pub const CONFIG_KEY_SYNC_MODE: &str = "startup.sync-mode";

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
