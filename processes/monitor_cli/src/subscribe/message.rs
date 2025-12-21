//! Message type for monitor_cli caryatid integration.
//!
//! This is a minimal message type that only needs to handle MonitorSnapshot
//! messages received from the message bus.

use caryatid_process::MonitorSnapshot;
use serde::{Deserialize, Serialize};

/// Message type for monitor_cli.
///
/// This enum represents the messages that monitor_cli can receive.
/// Currently only MonitorSnapshot is needed, but the enum structure
/// allows for future expansion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    /// A monitor snapshot received from the message bus.
    Monitor(MonitorSnapshot),

    /// JSON fallback for unknown message types.
    Json(serde_json::Value),
}

impl Default for Message {
    fn default() -> Self {
        Message::Json(serde_json::Value::Null)
    }
}

impl From<MonitorSnapshot> for Message {
    fn from(snapshot: MonitorSnapshot) -> Self {
        Message::Monitor(snapshot)
    }
}

impl Message {
    /// Try to extract a MonitorSnapshot from this message.
    pub fn as_monitor_snapshot(&self) -> Option<&MonitorSnapshot> {
        match self {
            Message::Monitor(snapshot) => Some(snapshot),
            Message::Json(value) => {
                // Try to deserialize from JSON if it's a JSON message
                // This handles the case where the snapshot was serialized as JSON
                None // Deserialization happens at receive time
            }
        }
    }

    /// Try to convert this message into a MonitorSnapshot.
    pub fn into_monitor_snapshot(self) -> Option<MonitorSnapshot> {
        match self {
            Message::Monitor(snapshot) => Some(snapshot),
            Message::Json(value) => {
                // Try to deserialize from JSON
                serde_json::from_value(value).ok()
            }
        }
    }
}
