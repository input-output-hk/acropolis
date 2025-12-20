use ratatui::style::{Color, Modifier, Style};

use crate::data::HealthStatus;

#[derive(Debug, Clone)]
pub struct Theme {
    pub highlight: Color,
    pub warning: Color,
    pub critical: Color,
    pub healthy: Color,
    pub border: Color,
    pub header: Style,
    pub selected: Style,
    pub tab_active: Style,
    pub tab_inactive: Style,
}

impl Theme {
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
        }
    }

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
