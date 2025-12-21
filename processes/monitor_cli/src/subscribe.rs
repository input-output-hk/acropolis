//! RabbitMQ subscription for receiving monitor snapshots.
//!
//! # Configuration
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
use anyhow::{Context, Result};
use config::{Config, Environment, File};
use futures_util::StreamExt;
use lapin::{
    options::{BasicConsumeOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    Connection, ConnectionProperties,
};
use std::path::Path;

/// Create a subscriber that connects directly to RabbitMQ.
///
/// # Arguments
///
/// * `config_path` - Path to config file with RabbitMQ settings
/// * `topic` - The topic pattern to subscribe to
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
    // Load config
    let config = Config::builder()
        .add_source(File::from(config_path))
        .add_source(Environment::with_prefix("CARYATID"))
        .build()?;

    // Extract RabbitMQ config - support both [rabbitmq] and [message-bus.*] formats
    let (url, exchange) = extract_rabbitmq_config(&config)?;

    // Connect to RabbitMQ
    let conn = Connection::connect(&url, ConnectionProperties::default())
        .await
        .context("Failed to connect to RabbitMQ")?;

    let channel = conn.create_channel().await?;

    // Declare a temporary exclusive queue
    let queue = channel
        .queue_declare(
            "",
            QueueDeclareOptions {
                exclusive: true,
                auto_delete: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;

    // Bind queue to the exchange with the topic pattern
    channel
        .queue_bind(
            queue.name().as_str(),
            &exchange,
            topic,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    // Start consuming
    let mut consumer = channel
        .basic_consume(
            queue.name().as_str(),
            "caryatid-doctor",
            BasicConsumeOptions {
                no_ack: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;

    // Create channel for forwarding to TUI
    let (tx, source) = ChannelSource::create(&format!("rabbitmq:{}", topic));

    // Spawn background task to read messages
    let handle = tokio::spawn(async move {
        while let Some(delivery) = consumer.next().await {
            match delivery {
                Ok(delivery) => {
                    // Try to deserialize as MonitorSnapshot
                    if let Ok(snapshot) = serde_json::from_slice::<MonitorSnapshot>(&delivery.data)
                    {
                        if tx.send(snapshot).is_err() {
                            // Receiver dropped
                            break;
                        }
                    }
                }
                Err(_) => {
                    // Connection error, exit
                    break;
                }
            }
        }
    });

    Ok((source, handle))
}

/// Extract RabbitMQ URL and exchange from config.
///
/// Supports two formats:
/// - `[rabbitmq]` with `url` and `exchange` fields
/// - `[message-bus.*]` with `class = "rabbit-mq"`, `url`, and `exchange` fields
fn extract_rabbitmq_config(config: &Config) -> Result<(String, String)> {
    // Try [rabbitmq] format first
    if let Ok(rabbitmq) = config.get_table("rabbitmq") {
        let url = rabbitmq
            .get("url")
            .and_then(|v| v.clone().into_string().ok())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' in [rabbitmq]"))?;
        let exchange = rabbitmq
            .get("exchange")
            .and_then(|v| v.clone().into_string().ok())
            .unwrap_or_else(|| "caryatid".to_string());
        return Ok((url, exchange));
    }

    // Try [message-bus.*] format
    if let Ok(message_bus) = config.get_table("message-bus") {
        for (_id, bus_conf) in message_bus {
            if let Ok(tbl) = bus_conf.into_table() {
                let class = tbl.get("class").and_then(|v| v.clone().into_string().ok());
                if class.as_deref() == Some("rabbit-mq") {
                    let url = tbl
                        .get("url")
                        .and_then(|v| v.clone().into_string().ok())
                        .ok_or_else(|| anyhow::anyhow!("Missing 'url' in rabbit-mq bus config"))?;
                    let exchange = tbl
                        .get("exchange")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_else(|| "caryatid".to_string());
                    return Ok((url, exchange));
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "Config must contain [rabbitmq] or [message-bus.*.class = \"rabbit-mq\"]"
    ))
}
