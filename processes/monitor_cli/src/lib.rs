//! # caryatid-doctor
//!
//! A diagnostic TUI and library for monitoring Caryatid message bus activity.
//!
//! This crate provides tools for visualizing and diagnosing the health of
//! modules communicating via the Caryatid message bus. It can receive monitor
//! snapshots from various sources (files, channels) and display them in an
//! interactive terminal UI.
//!
//! ## Features
//!
//! - **Summary view**: Overview of all modules with health status
//! - **Bottleneck detection**: Highlights topics with pending reads/writes
//! - **Data flow visualization**: Shows producer/consumer relationships
//! - **Historical tracking**: Sparklines and rate calculations
//!
//! ## Usage
//!
//! ### As a CLI tool
//!
//! ```bash
//! # Monitor a JSON file (produced by caryatid's Monitor)
//! caryatid-doctor --file monitor.json
//! ```
//!
//! ### As a library with file source
//!
//! ```ignore
//! use caryatid_doctor::{App, FileSource, Thresholds};
//!
//! let source = Box::new(FileSource::new("monitor.json"));
//! let app = App::new(source, Thresholds::default());
//! ```
//!
//! ### As a library with channel source (for message bus integration)
//!
//! ```ignore
//! use caryatid_doctor::{App, ChannelSource, MonitorSnapshot, Thresholds};
//!
//! // Create a channel for receiving snapshots
//! let (tx, source) = ChannelSource::create("rabbitmq://localhost");
//!
//! // Create the app
//! let app = App::new(Box::new(source), Thresholds::default());
//!
//! // Elsewhere, send snapshots from your message bus subscriber:
//! // tx.send(snapshot)?;
//! ```

pub mod app;
pub mod data;
pub mod events;
pub mod source;
pub mod ui;

// Re-export main types for convenience
pub use app::App;
pub use data::{HealthStatus, ModuleData, MonitorData, Thresholds, TopicRead, TopicWrite};
pub use source::{
    ChannelSource, DataSource, FileSource, MonitorSnapshot, SerializedModuleState,
    SerializedReadStreamState, SerializedWriteStreamState,
};
