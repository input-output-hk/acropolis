use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::data::duration::format_duration;

/// Render the module detail as a modal overlay with bordered sections
pub fn render_overlay(frame: &mut Frame, app: &App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let Some(module) = data.modules.get(app.selected_module_index) else {
        return;
    };

    // Calculate overlay size
    let overlay_width = 76u16;
    let overlay_height = (area.height * 85 / 100).min(40).max(15);
    let x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
    let y = area.y + (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    // Clear the area behind the overlay
    frame.render_widget(Clear, overlay_area);

    let inner_w = 72usize; // inner content width

    let mut lines: Vec<Line> = Vec::new();

    // Module header
    let health_label = match module.health {
        crate::data::HealthStatus::Healthy => "Healthy",
        crate::data::HealthStatus::Warning => "Warning",
        crate::data::HealthStatus::Critical => "Critical",
    };

    lines.push(make_box_top(inner_w, app.theme.highlight));
    lines.push(make_box_row(
        &module.name,
        inner_w,
        app.theme.highlight,
        true,
    ));
    let stats = format!(
        "Reads: {}   Writes: {}   {} {}",
        format_count(module.total_read),
        format_count(module.total_written),
        module.health.symbol(),
        health_label
    );
    lines.push(make_box_row(&stats, inner_w, app.theme.highlight, false));
    lines.push(make_box_bottom(inner_w, app.theme.highlight));
    lines.push(Line::from(""));

    // Reads section
    if !module.reads.is_empty() {
        let title = format!("Reads ({})", module.reads.len());
        lines.push(make_section_top(&title, inner_w, app));

        // Header
        lines.push(Line::from(vec![
            Span::styled(" │ ", Style::default().fg(app.theme.border)),
            Span::styled(
                format!(
                    "{:<26} {:>10} {:>10} {:>8} {:>8}",
                    "Topic", "Read", "Pending", "Unread", "Status"
                ),
                Style::default().add_modifier(Modifier::DIM),
            ),
            Span::styled(" │", Style::default().fg(app.theme.border)),
        ]));

        lines.push(make_separator(inner_w, app.theme.border));

        for r in &module.reads {
            let topic = truncate(&r.topic, 26);
            let read = format_count(r.read);
            let pending = r.pending_for.map(format_duration).unwrap_or("-".into());
            let unread = r.unread.map(format_count).unwrap_or("-".into());
            let status_style = app.theme.status_style(r.status);

            lines.push(Line::from(vec![
                Span::styled(" │ ", Style::default().fg(app.theme.border)),
                Span::raw(format!(
                    "{:<26} {:>10} {:>10} {:>8} ",
                    topic, read, pending, unread
                )),
                Span::styled(format!("{:^8}", r.status.symbol()), status_style),
                Span::styled(" │", Style::default().fg(app.theme.border)),
            ]));
        }

        lines.push(make_section_bottom(inner_w, app.theme.border));
        lines.push(Line::from(""));
    }

    // Writes section
    if !module.writes.is_empty() {
        let title = format!("Writes ({})", module.writes.len());
        lines.push(make_section_top(&title, inner_w, app));

        // Header
        lines.push(Line::from(vec![
            Span::styled(" │ ", Style::default().fg(app.theme.border)),
            Span::styled(
                format!(
                    "{:<26} {:>10} {:>10} {:>8} {:>8}",
                    "Topic", "Written", "Pending", "", "Status"
                ),
                Style::default().add_modifier(Modifier::DIM),
            ),
            Span::styled(" │", Style::default().fg(app.theme.border)),
        ]));

        lines.push(make_separator(inner_w, app.theme.border));

        for w in &module.writes {
            let topic = truncate(&w.topic, 26);
            let written = format_count(w.written);
            let pending = w.pending_for.map(format_duration).unwrap_or("-".into());
            let status_style = app.theme.status_style(w.status);

            lines.push(Line::from(vec![
                Span::styled(" │ ", Style::default().fg(app.theme.border)),
                Span::raw(format!(
                    "{:<26} {:>10} {:>10} {:>8} ",
                    topic, written, pending, ""
                )),
                Span::styled(format!("{:^8}", w.status.symbol()), status_style),
                Span::styled(" │", Style::default().fg(app.theme.border)),
            ]));
        }

        lines.push(make_section_bottom(inner_w, app.theme.border));
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "              Press Esc to close",
        Style::default().add_modifier(Modifier::DIM),
    )]));

    let block = Block::default()
        .title(" Module Detail ")
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.highlight));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, overlay_area);
}

fn make_box_top(width: usize, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!(" ╭{:─<w$}╮", "", w = width),
        Style::default().fg(color),
    )])
}

fn make_box_row(
    content: &str,
    width: usize,
    color: ratatui::style::Color,
    bold: bool,
) -> Line<'static> {
    let style = if bold {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let padding = width.saturating_sub(content.len());
    Line::from(vec![
        Span::styled(" │ ", Style::default().fg(color)),
        Span::styled(content.to_string(), style),
        Span::raw(format!("{:w$}", "", w = padding)),
        Span::styled(" │", Style::default().fg(color)),
    ])
}

fn make_box_bottom(width: usize, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!(" ╰{:─<w$}╯", "", w = width),
        Style::default().fg(color),
    )])
}

fn make_section_top(title: &str, width: usize, app: &App) -> Line<'static> {
    let title_display = format!(" {} ", title);
    let remaining = width.saturating_sub(title_display.len());
    Line::from(vec![
        Span::styled(" ╭", Style::default().fg(app.theme.border)),
        Span::styled(title_display, app.theme.header),
        Span::styled(
            format!("{:─<w$}╮", "", w = remaining),
            Style::default().fg(app.theme.border),
        ),
    ])
}

fn make_separator(width: usize, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!(" ├{:─<w$}┤", "", w = width),
        Style::default().fg(color),
    )])
}

fn make_section_bottom(width: usize, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!(" ╰{:─<w$}╯", "", w = width),
        Style::default().fg(color),
    )])
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
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
