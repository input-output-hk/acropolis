//! Data source abstraction for receiving monitor snapshots.
//!
//! This module provides a trait-based abstraction for receiving monitor data
//! from various sources (files, message bus channels, network streams, etc.).

mod channel;
mod file;
mod snapshot;
mod stream;

pub use channel::ChannelSource;
pub use file::FileSource;
pub use snapshot::{
    MonitorSnapshot, SerializedModuleState, SerializedReadStreamState, SerializedWriteStreamState,
};
pub use stream::StreamSource;

use std::fmt::Debug;

/// Trait for receiving monitor data from various sources.
///
/// Implementations of this trait provide monitor snapshots from different
/// backends - file polling, message bus subscriptions, or in-memory channels.
///
/// # Example
///
/// ```ignore
/// use caryatid_doctor::{FileSource, DataSource};
///
/// let source = FileSource::new("monitor.json");
/// if let Some(snapshot) = source.poll() {
///     println!("Got {} modules", snapshot.modules.len());
/// }
/// ```
pub trait DataSource: Send + Debug {
    /// Poll for the latest snapshot.
    ///
    /// Returns `Some(snapshot)` if new data is available, `None` otherwise.
    /// This method should be non-blocking.
    fn poll(&mut self) -> Option<MonitorSnapshot>;

    /// Returns a human-readable description of the source.
    ///
    /// Used for display in the TUI status bar.
    fn description(&self) -> &str;

    /// Check if the source has encountered an error.
    ///
    /// Returns the error message if an error occurred during the last poll.
    fn error(&self) -> Option<&str>;
}
