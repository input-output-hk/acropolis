//! Module-level configuration parsed from the `[module.script-eval-visualizer]`
//! section of the process config (e.g. `omnibus.toml`).

use acropolis_common::configuration::{get_string_flag, get_u64_flag};
use config::Config;

/// Default Caryatid topic to subscribe to for phase-2 evaluation results.
pub const DEFAULT_PHASE2_SUBSCRIBE_TOPIC: (&str, &str) =
    ("phase2-subscribe-topic", "cardano.utxo.phase2");

/// Default HTTP bind address — loopback to keep this operator-local by default.
pub const DEFAULT_BIND_ADDRESS: (&str, &str) = ("bind-address", "127.0.0.1");

/// Default HTTP port — sits clear of in-tree REST/MCP defaults.
pub const DEFAULT_BIND_PORT: (&str, u64) = ("bind-port", 8030);

/// Default network name used to build cexplorer.io links.
pub const DEFAULT_NETWORK: (&str, &str) = ("network", "mainnet");

/// Resolved module configuration.
#[derive(Debug, Clone)]
pub struct VisualizerConfig {
    /// Topic to subscribe to for `Phase2EvaluationResultsMessage`s.
    pub phase2_subscribe_topic: String,

    /// Bind address for the embedded HTTP server.
    pub bind_address: String,

    /// Bind port for the embedded HTTP server.
    pub bind_port: u16,

    /// Lowercase network name (`mainnet` / `preprod` / `preview` / …).
    pub network: String,
}

impl VisualizerConfig {
    /// Parse the module's section of the process config, applying the documented
    /// defaults when keys are absent.
    pub fn from_config(config: &Config) -> anyhow::Result<Self> {
        let port = get_u64_flag(config, DEFAULT_BIND_PORT);
        let bind_port: u16 =
            port.try_into().map_err(|_| anyhow::anyhow!("bind-port {port} does not fit in u16"))?;
        Ok(Self {
            phase2_subscribe_topic: get_string_flag(config, DEFAULT_PHASE2_SUBSCRIBE_TOPIC),
            bind_address: get_string_flag(config, DEFAULT_BIND_ADDRESS),
            bind_port,
            network: get_string_flag(config, DEFAULT_NETWORK).to_lowercase(),
        })
    }
}
