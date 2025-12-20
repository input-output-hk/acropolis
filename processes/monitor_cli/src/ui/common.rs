use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

use crate::app::{App, View};
use crate::data::HealthStatus;

/// Render the header bar with system health overview
pub fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let Some(ref data) = app.data else {
        let line = Line::from(vec![
            Span::styled(
                " ACROPOLIS MONITOR ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("| Loading..."),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    };

    // Count modules by health status
    let mut healthy = 0;
    let mut warning = 0;
    let mut critical = 0;

    for module in &data.modules {
        match module.health {
            HealthStatus::Healthy => healthy += 1,
            HealthStatus::Warning => warning += 1,
            HealthStatus::Critical => critical += 1,
        }
    }

    let total = data.modules.len();

    // Calculate total throughput
    let total_reads: u64 = data.modules.iter().map(|m| m.total_read).sum();
    let total_writes: u64 = data.modules.iter().map(|m| m.total_written).sum();

    // Overall status indicator
    let (status_icon, status_style) = if critical > 0 {
        ("●", app.theme.status_style(HealthStatus::Critical))
    } else if warning > 0 {
        ("●", app.theme.status_style(HealthStatus::Warning))
    } else {
        ("●", app.theme.status_style(HealthStatus::Healthy))
    };

    let line = Line::from(vec![
        Span::styled(format!(" {} ", status_icon), status_style),
        Span::styled("ACROPOLIS ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("│ "),
        Span::styled(
            format!("{}", healthy),
            Style::default().fg(app.theme.healthy),
        ),
        Span::raw(" ok "),
        if warning > 0 {
            Span::styled(
                format!("{}", warning),
                Style::default().fg(app.theme.warning),
            )
        } else {
            Span::styled("0", Style::default().add_modifier(Modifier::DIM))
        },
        Span::raw(" warn "),
        if critical > 0 {
            Span::styled(
                format!("{}", critical),
                Style::default().fg(app.theme.critical).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("0", Style::default().add_modifier(Modifier::DIM))
        },
        Span::raw(" crit │ "),
        Span::styled(
            format!("{}", total),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" modules │ "),
        Span::raw(format!(
            "R:{} W:{}",
            format_count(total_reads),
            format_count(total_writes)
        )),
    ]);

    frame.render_widget(Paragraph::new(line), area);
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

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
        Line::from(vec![Span::styled(
            " Navigation",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Tab/1-4   Switch views"),
        Line::from("  ↑/k ↓/j   Navigate list"),
        Line::from("  Enter     View detail"),
        Line::from("  Esc       Go back"),
        Line::from(""),
        Line::from(vec![Span::styled(
            " Summary View",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  /         Start filter"),
        Line::from("  c         Clear filter"),
        Line::from("  s         Cycle sort column"),
        Line::from("  S         Toggle sort direction"),
        Line::from(""),
        Line::from(vec![Span::styled(
            " General",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  r         Reload data"),
        Line::from("  e         Export to JSON"),
        Line::from("  q         Quit"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press any key to close",
            Style::default().add_modifier(Modifier::DIM),
        )]),
    ];

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.highlight));

    let paragraph = Paragraph::new(help_text).block(block);

    // Center the help overlay
    let help_width = 40;
    let help_height = 22;
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
