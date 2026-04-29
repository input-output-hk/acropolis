//! Acropolis Script Evaluation Visualizer module.
//!
//! Subscribes to per-transaction phase-2 Plutus script validation results
//! (published by `utxo_state` on `cardano.utxo.phase2`), fans each transaction's
//! outcomes out into one [`ScriptEvalEvent`] per script, broadcasts those events
//! to a [`tokio::sync::broadcast`] channel, and serves a small embedded HTML
//! page plus a `text/event-stream` endpoint that turns the broadcast into
//! Server-Sent-Events for connected browsers.
//!
//! See `specs/003-script-eval-visualizer/` for the spec, plan, and contracts.

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use acropolis_common::messages::{CardanoMessage, Message};
use anyhow::{anyhow, Result};
use caryatid_sdk::{module, Context};
use config::Config;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

mod cfg;
pub mod http;
pub mod stream;

use crate::stream::{fan_out, ScriptEvalEvent};

/// Capacity of the broadcast channel between the Caryatid subscription and
/// connected SSE clients. One slot per script evaluation event; sized to
/// absorb a worst-case mainnet block worth of evaluations even if a client
/// stalls briefly.
const BROADCAST_CAPACITY: usize = 4096;

/// Map a network name to the corresponding cexplorer.io base URL.
///
/// Mainnet is the canonical site; preprod / preview are the testnet
/// subdomains; anything else falls back to mainnet (best-effort, documented in
/// the spec assumptions).
pub fn cexplorer_base_url(network: &str) -> &'static str {
    match network.to_ascii_lowercase().as_str() {
        "mainnet" => "https://cexplorer.io",
        "preprod" => "https://preprod.cexplorer.io",
        "preview" => "https://preview.cexplorer.io",
        _ => "https://cexplorer.io",
    }
}

/// Caryatid module: phase-2 visualization.
#[module(
    message_type(Message),
    name = "script-eval-visualizer",
    description = "Real-time visualizer for phase-2 Plutus script evaluation results"
)]
pub struct ScriptEvalVisualizer;

impl ScriptEvalVisualizer {
    /// Module entrypoint.
    ///
    /// Parses configuration, sets up the fan-out broadcast channel, registers
    /// a Caryatid subscription on the configured topic, and spawns the HTTP
    /// server task.
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = cfg::VisualizerConfig::from_config(&config)?;
        info!(
            "script-eval-visualizer: subscribing to '{}', binding {}:{}",
            cfg.phase2_subscribe_topic, cfg.bind_address, cfg.bind_port
        );

        let (events_tx, _) = broadcast::channel::<ScriptEvalEvent>(BROADCAST_CAPACITY);

        // HTTP server task.
        let app_state = http::AppState {
            events: events_tx.clone(),
            cexplorer_base_url: cexplorer_base_url(&cfg.network).to_owned(),
            network: cfg.network.clone(),
        };
        let bind_ip = IpAddr::from_str(&cfg.bind_address)
            .map_err(|e| anyhow!("invalid bind-address '{}': {e}", cfg.bind_address))?;
        let bind_addr = SocketAddr::new(bind_ip, cfg.bind_port);
        tokio::spawn(async move {
            if let Err(e) = http::serve(bind_addr, app_state).await {
                error!("script-eval-visualizer HTTP server stopped: {e}");
            }
        });

        // Phase-2 subscription task.
        let mut subscription = context.subscribe(&cfg.phase2_subscribe_topic).await?;
        let next_id = Arc::new(AtomicU64::new(1));
        context.run(async move {
            loop {
                let read = match subscription.read().await {
                    Ok(pair) => pair,
                    Err(e) => {
                        warn!("script-eval-visualizer subscription read error: {e}");
                        return;
                    }
                };
                let (_topic, message) = read;
                let Message::Cardano((block_info, CardanoMessage::Phase2EvaluationResults(msg))) =
                    message.as_ref()
                else {
                    continue;
                };
                let events = fan_out(block_info, msg, &next_id);
                if events.is_empty() {
                    continue;
                }
                debug!(
                    "script-eval-visualizer fanning out {} events for tx {}",
                    events.len(),
                    hex::encode(msg.tx_hash.as_ref())
                );
                for event in events {
                    // ignore SendError (no subscribers) — the bus must keep flowing.
                    let _ = events_tx.send(event);
                }
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod cexplorer_url_tests {
    use super::cexplorer_base_url;

    #[test]
    fn mainnet_maps_to_canonical_site() {
        assert_eq!(cexplorer_base_url("mainnet"), "https://cexplorer.io");
        // Case-insensitive on the input — operators sometimes capitalize.
        assert_eq!(cexplorer_base_url("MAINNET"), "https://cexplorer.io");
    }

    #[test]
    fn preprod_and_preview_use_their_subdomains() {
        assert_eq!(
            cexplorer_base_url("preprod"),
            "https://preprod.cexplorer.io"
        );
        assert_eq!(
            cexplorer_base_url("preview"),
            "https://preview.cexplorer.io"
        );
    }

    #[test]
    fn unknown_networks_fall_back_to_mainnet() {
        assert_eq!(cexplorer_base_url("sancho"), "https://cexplorer.io");
        assert_eq!(cexplorer_base_url(""), "https://cexplorer.io");
        assert_eq!(cexplorer_base_url("local"), "https://cexplorer.io");
    }
}
