//! Caryatid message bus integration for monitor_cli.
//!
//! This module provides the ability to subscribe to monitor snapshots
//! published on the Caryatid message bus, enabling real-time monitoring
//! of a live system.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     Target Caryatid Process                     │
//! │  ┌─────────┐    ┌─────────┐    ┌─────────────────────────────┐ │
//! │  │ Module  │───▶│ Monitor │───▶│ Message Bus (topic publish) │ │
//! │  └─────────┘    └─────────┘    └──────────────┬──────────────┘ │
//! └───────────────────────────────────────────────┼────────────────┘
//!                                                 │
//!                                                 ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     monitor_cli Process                         │
//! │  ┌─────────────────────────────┐    ┌────────────────────────┐ │
//! │  │ Message Bus (topic subscribe)│───▶│ MonitorSubscriber      │ │
//! │  └─────────────────────────────┘    └───────────┬────────────┘ │
//! │                                                 │ watch::Sender │
//! │                                                 ▼               │
//! │                                    ┌────────────────────────┐  │
//! │                                    │ ChannelSource (TUI)    │  │
//! │                                    └────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```bash
//! # Subscribe to monitor snapshots from a caryatid process
//! caryatid-doctor --subscribe config.toml --topic caryatid.monitor
//! ```

mod message;
mod subscriber;

pub use message::Message;
pub use subscriber::MonitorSubscriber;

use crate::source::{ChannelSource, MonitorSnapshot};
use anyhow::Result;
use caryatid_process::Process;
use config::{Config, Environment, File};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::watch;

/// Run monitor_cli as a caryatid subscriber.
///
/// This creates a minimal caryatid process that subscribes to the monitor
/// topic and forwards snapshots to the TUI via a channel.
///
/// # Arguments
///
/// * `config_path` - Path to the caryatid config file (for message bus settings)
/// * `topic` - The topic to subscribe to for monitor snapshots
///
/// # Returns
///
/// A tuple of (sender, source) where:
/// - sender is used internally by the subscriber module
/// - source is a ChannelSource that can be used with the TUI
pub async fn create_subscriber(
    config_path: &Path,
    topic: &str,
) -> Result<(
    watch::Sender<MonitorSnapshot>,
    ChannelSource,
    tokio::task::JoinHandle<Result<()>>,
)> {
    // Create the channel for forwarding snapshots
    let (tx, source) = ChannelSource::create(&format!("subscribe:{}", topic));

    // Load config
    let config = Arc::new(
        Config::builder()
            .add_source(File::from(config_path))
            .add_source(Environment::with_prefix("CARYATID"))
            .build()?,
    );

    // Create the process
    let mut process = Process::<Message>::create(config).await;

    // Register our subscriber module with the channel sender
    let tx_clone = tx.clone();
    MonitorSubscriber::register_with_sender(&mut process, tx_clone, topic.to_string());

    // Start the process in a background task
    let handle = tokio::spawn(async move { process.run().await });

    Ok((tx, source, handle))
}
