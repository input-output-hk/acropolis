//! Data models and processing for monitor snapshots.
//!
//! This module handles the transformation of raw monitor snapshots into
//! structured, health-annotated data suitable for display.
//!
//! ## Submodules
//!
//! - [`duration`]: Parsing and formatting of duration strings (e.g., "1s", "500ms")
//! - [`flow`]: Data flow graph construction for visualizing producer/consumer relationships
//! - [`history`]: Historical tracking for sparklines and rate calculations
//! - [`monitor`]: Core data models ([`MonitorData`], [`ModuleData`], [`HealthStatus`])
//!
//! ## Data Flow
//!
//! ```text
//! MonitorSnapshot (raw JSON)
//!        │
//!        ▼
//! MonitorData::from_snapshot()
//!        │
//!        ├──▶ ModuleData (with health status computed from Thresholds)
//!        │
//!        └──▶ History::record() (for sparklines)
//! ```

pub mod duration;
pub mod flow;
pub mod history;
pub mod monitor;

pub use flow::DataFlowGraph;
pub use history::History;
pub use monitor::{
    HealthStatus, ModuleData, MonitorData, Thresholds, TopicRead, TopicWrite, UnhealthyTopic,
};
