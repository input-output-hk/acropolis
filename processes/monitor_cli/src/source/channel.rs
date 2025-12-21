//! Channel-based data source.
//!
//! Receives monitor snapshots via a tokio watch channel.
//! This is useful for message bus integration where snapshots
//! are pushed rather than polled from a file.

use tokio::sync::watch;

use super::{DataSource, MonitorSnapshot};

/// A data source that receives monitor snapshots via a channel.
///
/// This source is designed for integration with message bus systems.
/// The producer (e.g., a message bus subscriber) sends snapshots
/// through the channel, and this source provides them to the TUI.
///
/// # Example
///
/// ```
/// use caryatid_doctor::ChannelSource;
///
/// // Create a channel pair
/// let (tx, source) = ChannelSource::create("rabbitmq://localhost");
/// ```
#[derive(Debug)]
pub struct ChannelSource {
    receiver: watch::Receiver<MonitorSnapshot>,
    description: String,
    /// Track if we've returned the initial value yet
    initial_returned: bool,
}

impl ChannelSource {
    /// Create a new channel source.
    ///
    /// # Arguments
    ///
    /// * `receiver` - The receiving end of a watch channel
    /// * `source_description` - A description of where snapshots come from
    ///   (e.g., "rabbitmq://localhost", "nats://broker:4222")
    pub fn new(receiver: watch::Receiver<MonitorSnapshot>, source_description: &str) -> Self {
        let description = format!("channel: {}", source_description);
        Self {
            receiver,
            description,
            initial_returned: false,
        }
    }

    /// Create a channel pair for sending snapshots to a ChannelSource.
    ///
    /// Returns (sender, source) where the sender can be used to push
    /// snapshots and the source can be used with the Doctor TUI.
    pub fn create(source_description: &str) -> (watch::Sender<MonitorSnapshot>, Self) {
        let (tx, rx) = watch::channel(MonitorSnapshot::default());
        let source = Self::new(rx, source_description);
        (tx, source)
    }
}

impl DataSource for ChannelSource {
    fn poll(&mut self) -> Option<MonitorSnapshot> {
        // Return the initial value on first poll
        if !self.initial_returned {
            self.initial_returned = true;
            self.receiver.mark_changed();
        }

        // Check if there's a new value without blocking
        if self.receiver.has_changed().unwrap_or(false) {
            let snapshot = self.receiver.borrow_and_update().clone();
            Some(snapshot)
        } else {
            None
        }
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn error(&self) -> Option<&str> {
        // Channel sources don't have file-based errors
        // Connection errors would be handled by the message bus layer
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SerializedModuleState;
    use std::collections::BTreeMap;

    #[test]
    fn test_channel_source_poll() {
        let (tx, mut source) = ChannelSource::create("test");

        // Initially returns the default (empty) snapshot
        let snapshot = source.poll();
        assert!(snapshot.is_some());
        assert!(snapshot.unwrap().is_empty());

        // No change, so poll returns None
        assert!(source.poll().is_none());

        // Send a new snapshot
        let mut new_snapshot = MonitorSnapshot::new();
        new_snapshot.insert(
            "TestModule".to_string(),
            SerializedModuleState {
                reads: BTreeMap::new(),
                writes: BTreeMap::new(),
            },
        );
        tx.send(new_snapshot).unwrap();

        // Now poll returns the new snapshot
        let snapshot = source.poll();
        assert!(snapshot.is_some());
        assert_eq!(snapshot.unwrap().len(), 1);
    }
}
