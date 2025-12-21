//! Subscriber module for receiving MonitorSnapshot messages.
//!
//! This module subscribes to a topic on the Caryatid message bus and
//! forwards received MonitorSnapshot messages to the TUI via a watch channel.

use super::Message;
use crate::source::MonitorSnapshot;
use anyhow::Result;
use async_trait::async_trait;
use caryatid_sdk::{Context, Module, ModuleRegistry};
use config::Config;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

/// A module that subscribes to monitor snapshots and forwards them to a channel.
pub struct MonitorSubscriber {
    /// The watch channel sender for forwarding snapshots to the TUI.
    sender: watch::Sender<MonitorSnapshot>,
    /// The topic to subscribe to.
    topic: String,
}

impl MonitorSubscriber {
    /// Create a new MonitorSubscriber.
    pub fn new(sender: watch::Sender<MonitorSnapshot>, topic: String) -> Self {
        Self { sender, topic }
    }

    /// Register this module with a caryatid process, providing the sender and topic.
    pub fn register_with_sender(
        registry: &mut dyn ModuleRegistry<Message>,
        sender: watch::Sender<MonitorSnapshot>,
        topic: String,
    ) {
        let module = Arc::new(Self::new(sender, topic));
        registry.register(module);
    }
}

#[async_trait]
impl Module<Message> for MonitorSubscriber {
    async fn init(&self, context: Arc<Context<Message>>, _config: Arc<Config>) -> Result<()> {
        let topic = self.topic.clone();
        let sender = self.sender.clone();

        info!("MonitorSubscriber subscribing to topic: {}", topic);

        // Subscribe to the monitor topic
        let mut subscription = context.subscribe(&topic).await?;

        // Run the subscription loop
        context.run(async move {
            loop {
                match subscription.read().await {
                    Ok((_, message)) => {
                        // Try to extract the MonitorSnapshot from the message
                        // Note: caryatid_process::MonitorSnapshot is a newtype around BTreeMap,
                        // while our MonitorSnapshot is directly a BTreeMap type alias.
                        let snapshot: Option<MonitorSnapshot> = match message.as_ref() {
                            Message::Monitor(caryatid_snapshot) => {
                                // Convert from caryatid's MonitorSnapshot (newtype) to ours
                                Some(
                                    caryatid_snapshot
                                        .0
                                        .iter()
                                        .map(|(k, v)| {
                                            (
                                                k.clone(),
                                                crate::source::SerializedModuleState {
                                                    reads: v
                                                        .reads
                                                        .iter()
                                                        .map(|(topic, state)| {
                                                            (
                                                                topic.clone(),
                                                                crate::source::SerializedReadStreamState {
                                                                    read: state.read,
                                                                    unread: state.unread,
                                                                    pending_for: state.pending_for.clone(),
                                                                },
                                                            )
                                                        })
                                                        .collect(),
                                                    writes: v
                                                        .writes
                                                        .iter()
                                                        .map(|(topic, state)| {
                                                            (
                                                                topic.clone(),
                                                                crate::source::SerializedWriteStreamState {
                                                                    written: state.written,
                                                                    pending_for: state.pending_for.clone(),
                                                                },
                                                            )
                                                        })
                                                        .collect(),
                                                },
                                            )
                                        })
                                        .collect(),
                                )
                            }
                            Message::Json(value) => {
                                // Try to deserialize from JSON directly
                                match serde_json::from_value::<MonitorSnapshot>(value.clone()) {
                                    Ok(snapshot) => Some(snapshot),
                                    Err(e) => {
                                        warn!(
                                            "Failed to deserialize MonitorSnapshot from JSON: {}",
                                            e
                                        );
                                        None
                                    }
                                }
                            }
                        };

                        if let Some(snapshot) = snapshot {
                            debug!("Received MonitorSnapshot with {} modules", snapshot.len());
                            // Forward to the TUI
                            if sender.send(snapshot).is_err() {
                                // Receiver dropped, exit
                                info!("TUI receiver dropped, stopping subscriber");
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error reading from subscription: {}", e);
                        return;
                    }
                }
            }
        });

        Ok(())
    }

    fn get_name(&self) -> &'static str {
        "monitor-subscriber"
    }

    fn get_description(&self) -> &'static str {
        "Subscribes to monitor snapshots and forwards them to the TUI"
    }
}

impl std::fmt::Debug for MonitorSubscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonitorSubscriber").field("topic", &self.topic).finish()
    }
}
