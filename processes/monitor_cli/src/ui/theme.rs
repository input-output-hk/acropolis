//! Theme configuration for the TUI.
//!
//! Supports light and dark themes with automatic terminal detection.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::block::BorderType;

use crate::data::HealthStatus;

/// Color and style theme for the TUI.
///
/// Use [`Theme::auto_detect()`] for automatic theme selection based on
/// terminal background, or [`Theme::dark()`]/[`Theme::light()`] explicitly.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Accent color for highlights and active elements.
    pub highlight: Color,
    /// Color for warning-level health status.
    pub warning: Color,
    /// Color for critical-level health status.
    pub critical: Color,
    /// Color for healthy status.
    pub healthy: Color,
    /// Color for borders and separators.
    pub border: Color,
    /// Style for header rows in tables.
    pub header: Style,
    /// Style for selected/highlighted rows.
    pub selected: Style,
    /// Style for the active tab.
    pub tab_active: Style,
    /// Style for inactive tabs.
    pub tab_inactive: Style,
    /// Border style (rounded, plain, etc.).
    pub border_type: BorderType,
}

impl Theme {
    /// Create a dark theme suitable for dark terminal backgrounds.
    pub fn dark() -> Self {
        Self {
            highlight: Color::Cyan,
            warning: Color::Yellow,
            critical: Color::Red,
            healthy: Color::Green,
            border: Color::Gray,
            header: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            selected: Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD),
            tab_active: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            tab_inactive: Style::default().fg(Color::Gray),
            border_type: BorderType::Rounded,
        }
    }

    /// Create a light theme suitable for light terminal backgrounds.
    pub fn light() -> Self {
        Self {
            highlight: Color::Blue,
            warning: Color::Yellow,
            critical: Color::Red,
            healthy: Color::Green,
            border: Color::DarkGray,
            header: Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            selected: Style::default().bg(Color::LightBlue).add_modifier(Modifier::BOLD),
            tab_active: Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            tab_inactive: Style::default().fg(Color::DarkGray),
            border_type: BorderType::Rounded,
        }
    }

    /// Auto-detect based on terminal background
    pub fn auto_detect() -> Self {
        // Use terminal-light crate to detect background luminance
        match terminal_light::luma() {
            Ok(luma) if luma > 0.5 => Self::light(),
            _ => Self::dark(),
        }
    }

    /// Get style for a health status
    pub fn status_style(&self, status: HealthStatus) -> Style {
        match status {
            HealthStatus::Healthy => Style::default().fg(self.healthy),
            HealthStatus::Warning => Style::default().fg(self.warning),
            HealthStatus::Critical => {
                Style::default().fg(self.critical).add_modifier(Modifier::BOLD)
            }
        }
    }
}
