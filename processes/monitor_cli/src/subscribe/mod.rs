//! Caryatid message bus integration for monitor_cli.
//!
//! This module provides the ability to subscribe to monitor snapshots
//! published on the Caryatid message bus, enabling real-time monitoring
//! of a live system.
//!
//! # Configuration
//!
//! The subscribe feature requires a simple config file with RabbitMQ settings:
//!
//! ```toml
//! [rabbitmq]
//! url = "amqp://127.0.0.1:5672/%2f"
//! exchange = "caryatid"
//! ```
//!
//! # Usage
//!
//! ```bash
//! caryatid-doctor --subscribe config.toml --topic caryatid.monitor.snapshot
//! ```

use crate::source::{ChannelSource, MonitorSnapshot};
use anyhow::Result;
use caryatid_sdk::MessageBus;
use config::{Config, Environment, File};
use std::path::Path;

// Re-export for use in main.rs
pub use caryatid_process::MonitorSnapshot as CaryatidMonitorSnapshot;

/// Create a subscriber that connects directly to RabbitMQ.
///
/// This bypasses the full caryatid Process machinery and just uses
/// the RabbitMQ bus directly for subscribing to topics.
///
/// # Arguments
///
/// * `config_path` - Path to config file with RabbitMQ settings
/// * `topic` - The topic to subscribe to
///
/// # Returns
///
/// A tuple of (source, handle) where:
/// - source is a ChannelSource for the TUI
/// - handle is the background task reading from RabbitMQ
pub async fn create_subscriber(
    config_path: &Path,
    topic: &str,
) -> Result<(ChannelSource, tokio::task::JoinHandle<()>)> {
    use caryatid_process::rabbit_mq_bus::RabbitMQBus;

    // Load config
    let config = Config::builder()
        .add_source(File::from(config_path))
        .add_source(Environment::with_prefix("CARYATID"))
        .build()?;

    // Extract RabbitMQ config - support both [rabbitmq] and [message-bus.external] formats
    let bus_config = if let Ok(rabbitmq) = config.get_table("rabbitmq") {
        caryatid_sdk::config::config_from_value(rabbitmq)
    } else if let Ok(message_bus) = config.get_table("message-bus") {
        // Find the first rabbit-mq bus
        let mut found = None;
        for (_id, bus_conf) in message_bus {
            if let Ok(tbl) = bus_conf.into_table() {
                let cfg = caryatid_sdk::config::config_from_value(tbl);
                if cfg.get_string("class").ok() == Some("rabbit-mq".to_string()) {
                    found = Some(cfg);
                    break;
                }
            }
        }
        found.ok_or_else(|| anyhow::anyhow!("No rabbit-mq bus found in config"))?
    } else {
        return Err(anyhow::anyhow!(
            "Config must contain [rabbitmq] or [message-bus.*.class = \"rabbit-mq\"]"
        ));
    };

    // Create the RabbitMQ bus directly
    let bus = RabbitMQBus::<serde_json::Value>::new(&bus_config).await?;

    // Subscribe to the topic
    let mut subscription = bus.subscribe(topic).await?;

    // Create channel for forwarding to TUI
    let (tx, source) = ChannelSource::create(&format!("rabbitmq:{}", topic));

    // Spawn background task to read messages
    let handle = tokio::spawn(async move {
        loop {
            match subscription.read().await {
                Ok((_, message)) => {
                    // Try to deserialize as MonitorSnapshot
                    match serde_json::from_value::<MonitorSnapshot>(message.as_ref().clone()) {
                        Ok(snapshot) => {
                            if tx.send(snapshot).is_err() {
                                // Receiver dropped
                                break;
                            }
                        }
                        Err(_e) => {
                            // Not a valid snapshot, skip
                        }
                    }
                }
                Err(_e) => {
                    // Connection error, exit
                    break;
                }
            }
        }
    });

    Ok((source, handle))
}
