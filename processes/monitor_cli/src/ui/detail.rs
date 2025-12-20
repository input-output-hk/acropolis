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

    // Fixed column widths for consistent layout
    const TOPIC_W: usize = 28;
    const COUNT_W: usize = 10;
    const PENDING_W: usize = 10;
    const UNREAD_W: usize = 8;
    const STATUS_W: usize = 6;

    // Total inner width = columns + separators + padding
    let inner_content_width = TOPIC_W + COUNT_W + PENDING_W + UNREAD_W + STATUS_W + 4; // +4 for spacing

    // Calculate overlay size
    let overlay_width = (inner_content_width + 6) as u16; // +6 for borders and padding
    let overlay_height = (area.height * 85 / 100).min(40).max(15);
    let x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
    let y = area.y + (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    // Clear the area behind the overlay
    frame.render_widget(Clear, overlay_area);

    let box_inner = inner_content_width;

    // Build content lines
    let mut lines: Vec<Line> = Vec::new();

    // Module header box
    let health_label = match module.health {
        crate::data::HealthStatus::Healthy => "Healthy",
        crate::data::HealthStatus::Warning => "Warning",
        crate::data::HealthStatus::Critical => "Critical",
    };

    lines.push(Line::from(vec![Span::styled(
        format!(" ╭{:─<w$}╮", "", w = box_inner),
        Style::default().fg(app.theme.highlight),
    )]));

    lines.push(Line::from(vec![
        Span::styled(" │ ", Style::default().fg(app.theme.highlight)),
        Span::styled(
            module.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "{:w$}",
            "",
            w = box_inner.saturating_sub(module.name.len() + 1)
        )),
        Span::styled("│", Style::default().fg(app.theme.highlight)),
    ]));

    let stats_line = format!(
        "Reads: {}  Writes: {}  {} {}",
        format_count(module.total_read),
        format_count(module.total_written),
        module.health.symbol(),
        health_label
    );
    lines.push(Line::from(vec![
        Span::styled(" │ ", Style::default().fg(app.theme.highlight)),
        Span::raw(stats_line.clone()),
        Span::raw(format!(
            "{:w$}",
            "",
            w = box_inner.saturating_sub(stats_line.len() + 1)
        )),
        Span::styled("│", Style::default().fg(app.theme.highlight)),
    ]));

    lines.push(Line::from(vec![Span::styled(
        format!(" ╰{:─<w$}╯", "", w = box_inner),
        Style::default().fg(app.theme.highlight),
    )]));

    lines.push(Line::from(""));

    // Reads section
    if !module.reads.is_empty() {
        let title = format!(" Reads ({}) ", module.reads.len());
        let title_pad = box_inner.saturating_sub(title.len());

        lines.push(Line::from(vec![
            Span::styled(" ╭", Style::default().fg(app.theme.border)),
            Span::styled(title, app.theme.header),
            Span::styled(
                format!("{:─<w$}╮", "", w = title_pad),
                Style::default().fg(app.theme.border),
            ),
        ]));

        // Header row
        lines.push(Line::from(vec![
            Span::styled(" │ ", Style::default().fg(app.theme.border)),
            Span::styled(
                format!(
                    "{:<TOPIC_W$} {:>COUNT_W$} {:>PENDING_W$} {:>UNREAD_W$} {:>STATUS_W$}",
                    "Topic",
                    "Read",
                    "Pending",
                    "Unread",
                    "Status",
                    TOPIC_W = TOPIC_W,
                    COUNT_W = COUNT_W,
                    PENDING_W = PENDING_W,
                    UNREAD_W = UNREAD_W,
                    STATUS_W = STATUS_W
                ),
                Style::default().add_modifier(Modifier::DIM),
            ),
            Span::styled(" │", Style::default().fg(app.theme.border)),
        ]));

        // Separator
        lines.push(Line::from(vec![Span::styled(
            format!(" ├{:─<w$}┤", "", w = box_inner),
            Style::default().fg(app.theme.border),
        )]));

        for r in &module.reads {
            let status_style = app.theme.status_style(r.status);
            let pending = r.pending_for.map(format_duration).unwrap_or_else(|| "-".to_string());
            let unread = r.unread.map(|u| format_count(u)).unwrap_or_else(|| "-".to_string());
            let topic = truncate(&r.topic, TOPIC_W);

            lines.push(Line::from(vec![
                Span::styled(" │ ", Style::default().fg(app.theme.border)),
                Span::raw(format!(
                    "{:<TOPIC_W$} {:>COUNT_W$} {:>PENDING_W$} {:>UNREAD_W$} ",
                    topic,
                    format_count(r.read),
                    pending,
                    unread,
                    TOPIC_W = TOPIC_W,
                    COUNT_W = COUNT_W,
                    PENDING_W = PENDING_W,
                    UNREAD_W = UNREAD_W
                )),
                Span::styled(
                    format!("{:^STATUS_W$}", r.status.symbol(), STATUS_W = STATUS_W),
                    status_style,
                ),
                Span::styled(" │", Style::default().fg(app.theme.border)),
            ]));
        }

        lines.push(Line::from(vec![Span::styled(
            format!(" ╰{:─<w$}╯", "", w = box_inner),
            Style::default().fg(app.theme.border),
        )]));

        lines.push(Line::from(""));
    }

    // Writes section
    if !module.writes.is_empty() {
        let title = format!(" Writes ({}) ", module.writes.len());
        let title_pad = box_inner.saturating_sub(title.len());

        lines.push(Line::from(vec![
            Span::styled(" ╭", Style::default().fg(app.theme.border)),
            Span::styled(title, app.theme.header),
            Span::styled(
                format!("{:─<w$}╮", "", w = title_pad),
                Style::default().fg(app.theme.border),
            ),
        ]));

        // Header row - writes don't have unread
        lines.push(Line::from(vec![
            Span::styled(" │ ", Style::default().fg(app.theme.border)),
            Span::styled(
                format!(
                    "{:<TOPIC_W$} {:>COUNT_W$} {:>PENDING_W$} {:>UNREAD_W$} {:>STATUS_W$}",
                    "Topic",
                    "Written",
                    "Pending",
                    "",
                    "Status",
                    TOPIC_W = TOPIC_W,
                    COUNT_W = COUNT_W,
                    PENDING_W = PENDING_W,
                    UNREAD_W = UNREAD_W,
                    STATUS_W = STATUS_W
                ),
                Style::default().add_modifier(Modifier::DIM),
            ),
            Span::styled(" │", Style::default().fg(app.theme.border)),
        ]));

        // Separator
        lines.push(Line::from(vec![Span::styled(
            format!(" ├{:─<w$}┤", "", w = box_inner),
            Style::default().fg(app.theme.border),
        )]));

        for w in &module.writes {
            let status_style = app.theme.status_style(w.status);
            let pending = w.pending_for.map(format_duration).unwrap_or_else(|| "-".to_string());
            let topic = truncate(&w.topic, TOPIC_W);

            lines.push(Line::from(vec![
                Span::styled(" │ ", Style::default().fg(app.theme.border)),
                Span::raw(format!(
                    "{:<TOPIC_W$} {:>COUNT_W$} {:>PENDING_W$} {:>UNREAD_W$} ",
                    topic,
                    format_count(w.written),
                    pending,
                    "",
                    TOPIC_W = TOPIC_W,
                    COUNT_W = COUNT_W,
                    PENDING_W = PENDING_W,
                    UNREAD_W = UNREAD_W
                )),
                Span::styled(
                    format!("{:^STATUS_W$}", w.status.symbol(), STATUS_W = STATUS_W),
                    status_style,
                ),
                Span::styled(" │", Style::default().fg(app.theme.border)),
            ]));
        }

        lines.push(Line::from(vec![Span::styled(
            format!(" ╰{:─<w$}╯", "", w = box_inner),
            Style::default().fg(app.theme.border),
        )]));
    }

    // Footer hint
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "                Press Esc to close",
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
