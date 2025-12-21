// Library crate: public API items may not be used by the binary
#![allow(unused)]

//! # caryatid-doctor
//!
//! A diagnostic TUI and library for monitoring Caryatid message bus activity.
//!
//! This crate provides tools for visualizing and diagnosing the health of
//! modules communicating via the Caryatid message bus. It can receive monitor
//! snapshots from various sources (files, channels, network streams) and
//! display them in an interactive terminal UI.
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
//!
//! # Monitor via TCP connection
//! caryatid-doctor --connect localhost:9090
//! ```
//!
//! ### As a library with file source
//!
//! ```
//! use caryatid_doctor::{App, FileSource, Thresholds};
//!
//! let source = Box::new(FileSource::new("monitor.json"));
//! let app = App::new(source, Thresholds::default());
//! ```
//!
//! ### As a library with stream source (TCP, etc.)
//!
//! ```no_run
//! use std::io::Cursor;
//! use caryatid_doctor::{App, StreamSource, Thresholds};
//!
//! # tokio_test::block_on(async {
//! // Example with a cursor (in practice, use TcpStream)
//! let data = b"{}\n";
//! let stream = Cursor::new(data.to_vec());
//! let source = StreamSource::spawn(stream, "example");
//! let app = App::new(Box::new(source), Thresholds::default());
//! # });
//! ```
//!
//! ### As a library with channel source (for message bus integration)
//!
//! ```
//! use caryatid_doctor::{App, ChannelSource, Thresholds};
//!
//! // Create a channel for receiving snapshots
//! let (tx, source) = ChannelSource::create("rabbitmq://localhost");
//!
//! // Create the app
//! let app = App::new(Box::new(source), Thresholds::default());
//! ```
//!
//! ### Bridging from a message bus
//!
//! ```no_run
//! use caryatid_doctor::StreamSource;
//! use tokio::sync::mpsc;
//!
//! # tokio_test::block_on(async {
//! // Create a bytes channel
//! let (tx, rx) = mpsc::channel::<Vec<u8>>(16);
//! let source = StreamSource::from_bytes_channel(rx, "rabbitmq");
//! # });
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
    SerializedReadStreamState, SerializedWriteStreamState, StreamSource,
};
