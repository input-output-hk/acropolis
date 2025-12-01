use config::Config;
use serde::Deserialize;
use std::fmt::{Display, Formatter, Result};

pub const CONFIG_KEY_STARTUP_METHOD: &str = "startup.method";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupMethod {
    Mithril,
    Snapshot,
}

impl StartupMethod {
    pub fn from_config(config: &Config) -> Self {
        config.get::<StartupMethod>(CONFIG_KEY_STARTUP_METHOD).unwrap_or(StartupMethod::Mithril)
    }

    pub fn is_mithril(&self) -> bool {
        matches!(self, StartupMethod::Mithril)
    }

    pub fn is_snapshot(&self) -> bool {
        matches!(self, StartupMethod::Snapshot)
    }
}

impl Display for StartupMethod {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            StartupMethod::Mithril => write!(f, "mithril"),
            StartupMethod::Snapshot => write!(f, "snapshot"),
        }
    }
}
