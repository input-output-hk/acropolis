//! Shared types for monitor snapshots.
//!
//! These types match the serialization format produced by caryatid's Monitor.
//! They serve as the common data format between the monitor producer and
//! this doctor/viewer consumer.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A complete snapshot of monitor state.
///
/// This is the top-level structure that maps module names to their state.
/// It matches the JSON format written by caryatid's Monitor.
pub type MonitorSnapshot = BTreeMap<String, SerializedModuleState>;

/// State for a single module, containing its read and write stream states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedModuleState {
    /// Read stream states, keyed by topic name.
    pub reads: BTreeMap<String, SerializedReadStreamState>,
    /// Write stream states, keyed by topic name.
    pub writes: BTreeMap<String, SerializedWriteStreamState>,
}

/// State for a read stream (subscription).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedReadStreamState {
    /// Number of messages read from this topic.
    pub read: u64,

    /// Number of unread messages available on this topic.
    /// This is based on messages published by other modules.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread: Option<u64>,

    /// How long this module has been waiting to read from this topic.
    /// Format: Duration debug string (e.g., "1.234s", "500ms").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_for: Option<String>,
}

/// State for a write stream (publisher).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedWriteStreamState {
    /// Number of messages written to this topic.
    pub written: u64,

    /// How long this module has been waiting to write to this topic.
    /// If set, the topic is congested (subscriber not reading).
    /// Format: Duration debug string (e.g., "1.234s", "500ms").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_for: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_snapshot() {
        let json = r#"{
            "MyModule": {
                "reads": {
                    "input_topic": {
                        "read": 100,
                        "unread": 5,
                        "pending_for": "1.5s"
                    }
                },
                "writes": {
                    "output_topic": {
                        "written": 50
                    }
                }
            }
        }"#;

        let snapshot: MonitorSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snapshot.len(), 1);

        let module = snapshot.get("MyModule").unwrap();
        assert_eq!(module.reads.len(), 1);
        assert_eq!(module.writes.len(), 1);

        let read = module.reads.get("input_topic").unwrap();
        assert_eq!(read.read, 100);
        assert_eq!(read.unread, Some(5));
        assert_eq!(read.pending_for, Some("1.5s".to_string()));

        let write = module.writes.get("output_topic").unwrap();
        assert_eq!(write.written, 50);
        assert!(write.pending_for.is_none());
    }
}
