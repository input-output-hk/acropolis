//! Terminal UI rendering using ratatui.
//!
//! This module contains all the view-specific rendering logic for the TUI.
//! Each view is implemented in its own submodule with a `render` function.
//!
//! ## Submodules
//!
//! - [`summary`]: Main overview table showing all modules with health status
//! - [`bottleneck`]: Filtered view of topics that have pending reads/writes
//! - [`flow`]: Adjacency matrix visualization of producer/consumer relationships
//! - [`detail`]: Modal overlay showing detailed module information
//! - [`common`]: Shared components (header, tabs, status bar, help overlay)
//! - [`theme`]: Light/dark theme support with terminal auto-detection
//!
//! ## Rendering Architecture
//!
//! The main loop in `main.rs` calls into these modules based on the current view:
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │ Header (common::render_header)       │
//! ├──────────────────────────────────────┤
//! │ Tabs (common::render_tabs)           │
//! ├──────────────────────────────────────┤
//! │                                      │
//! │ View Content                         │
//! │ (summary/bottleneck/flow::render)    │
//! │                                      │
//! ├──────────────────────────────────────┤
//! │ Status Bar (common::render_status)   │
//! └──────────────────────────────────────┘
//!         ↑
//!    Overlays rendered on top:
//!    - detail::render_overlay
//!    - common::render_help
//! ```

pub mod bottleneck;
pub mod common;
pub mod detail;
pub mod flow;
pub mod summary;
pub mod theme;

pub use bottleneck::BottleneckSortColumn;
pub use theme::Theme;
