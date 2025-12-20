use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

use crate::app::{App, View};

/// Render the tab bar at the top
pub fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = vec![
        Line::from(" 1:Summary "),
        Line::from(" 2:Bottlenecks "),
        Line::from(" 3:Detail "),
        Line::from(" 4:Flow "),
    ];

    let selected = match app.current_view {
        View::Summary => 0,
        View::Bottleneck => 1,
        View::ModuleDetail => 2,
        View::DataFlow => 3,
    };

    let tabs = Tabs::new(titles)
        .select(selected)
        .style(app.theme.tab_inactive)
        .highlight_style(app.theme.tab_active)
        .divider("|");

    frame.render_widget(tabs, area);
}

/// Render the status bar at the bottom
pub fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let status = if let Some(ref data) = app.data {
        let elapsed = data.last_updated.elapsed();
        let healthy_count =
            data.modules.iter().filter(|m| m.health == crate::data::HealthStatus::Healthy).count();
        let total_count = data.modules.len();

        format!(
            " {} modules ({} healthy) | Updated {:.1}s ago | q:quit Tab:switch ?:help",
            total_count,
            healthy_count,
            elapsed.as_secs_f64()
        )
    } else if let Some(ref err) = app.load_error {
        format!(" Error: {} | q:quit r:retry", err)
    } else {
        " Loading... | q:quit".to_string()
    };

    let paragraph = Paragraph::new(status).style(Style::default().add_modifier(Modifier::DIM));

    frame.render_widget(paragraph, area);
}

/// Render the help overlay
pub fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let help_text = vec![
        Line::from(vec![Span::styled("Keyboard Shortcuts", app.theme.header)]),
        Line::from(""),
        Line::from("  q         Quit"),
        Line::from("  Tab       Next view"),
        Line::from("  Shift+Tab Previous view"),
        Line::from("  1-4       Jump to view"),
        Line::from("  Up/k      Select previous"),
        Line::from("  Down/j    Select next"),
        Line::from("  Enter     View module detail"),
        Line::from("  Esc       Go back"),
        Line::from("  r         Reload data"),
        Line::from("  ?         Toggle this help"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press any key to close",
            Style::default().add_modifier(Modifier::DIM),
        )]),
    ];

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.highlight));

    let paragraph = Paragraph::new(help_text).block(block);

    // Center the help overlay
    let help_width = 40;
    let help_height = 16;
    let x = area.x + (area.width.saturating_sub(help_width)) / 2;
    let y = area.y + (area.height.saturating_sub(help_height)) / 2;
    let help_area = Rect::new(
        x,
        y,
        help_width.min(area.width),
        help_height.min(area.height),
    );

    // Clear the area behind the help
    frame.render_widget(ratatui::widgets::Clear, help_area);
    frame.render_widget(paragraph, help_area);
}
