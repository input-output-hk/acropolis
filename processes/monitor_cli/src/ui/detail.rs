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

    // Calculate overlay size (85% of screen, max dimensions)
    let overlay_width = (area.width * 85 / 100).min(100).max(70);
    let overlay_height = (area.height * 85 / 100).min(40).max(15);
    let x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
    let y = area.y + (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    // Clear the area behind the overlay
    frame.render_widget(Clear, overlay_area);

    let inner_width = overlay_width.saturating_sub(4) as usize;

    // Build content lines
    let mut lines: Vec<Line> = Vec::new();

    // Module header box
    let health_label = match module.health {
        crate::data::HealthStatus::Healthy => "Healthy",
        crate::data::HealthStatus::Warning => "Warning",
        crate::data::HealthStatus::Critical => "Critical",
    };
    let header_text = format!(
        "Total Reads: {}    Total Writes: {}    {} {}",
        format_count(module.total_read),
        format_count(module.total_written),
        module.health.symbol(),
        health_label
    );

    lines.push(Line::from(vec![Span::styled(
        format!(" ╭{:─<width$}╮", "", width = inner_width - 2),
        Style::default().fg(app.theme.highlight),
    )]));

    let name_padding = inner_width.saturating_sub(module.name.len() + 4);
    lines.push(Line::from(vec![
        Span::styled(" │ ".to_string(), Style::default().fg(app.theme.highlight)),
        Span::styled(
            module.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{:width$}", "", width = name_padding)),
        Span::styled(" │".to_string(), Style::default().fg(app.theme.highlight)),
    ]));

    let header_padding = inner_width.saturating_sub(header_text.len() + 4);
    lines.push(Line::from(vec![
        Span::styled(" │ ".to_string(), Style::default().fg(app.theme.highlight)),
        Span::raw(header_text),
        Span::raw(format!("{:width$}", "", width = header_padding)),
        Span::styled(" │".to_string(), Style::default().fg(app.theme.highlight)),
    ]));

    lines.push(Line::from(vec![Span::styled(
        format!(" ╰{:─<width$}╯", "", width = inner_width - 2),
        Style::default().fg(app.theme.highlight),
    )]));

    lines.push(Line::from(""));

    // Reads section with border
    if !module.reads.is_empty() {
        let reads_title = format!(" Reads ({}) ", module.reads.len());
        let title_padding = inner_width.saturating_sub(reads_title.len() + 2);

        lines.push(Line::from(vec![
            Span::styled(" ╭".to_string(), Style::default().fg(app.theme.border)),
            Span::styled(reads_title, app.theme.header),
            Span::styled(
                format!("{:─<width$}╮", "", width = title_padding),
                Style::default().fg(app.theme.border),
            ),
        ]));

        // Header row
        lines.push(Line::from(vec![
            Span::styled(" │  ".to_string(), Style::default().fg(app.theme.border)),
            Span::styled(
                format!(
                    "{:<30} {:>8} {:>10} {:>8} {:>6}",
                    "Topic", "Read", "Pending", "Unread", "Status"
                ),
                Style::default().add_modifier(Modifier::DIM),
            ),
            Span::styled(" │".to_string(), Style::default().fg(app.theme.border)),
        ]));

        // Separator
        lines.push(Line::from(vec![Span::styled(
            format!(" ├{:─<width$}┤", "", width = inner_width - 2),
            Style::default().fg(app.theme.border),
        )]));

        for r in &module.reads {
            let status_style = app.theme.status_style(r.status);
            let pending = r.pending_for.map(format_duration).unwrap_or_else(|| "-".to_string());
            let unread = r.unread.map(|u| format_count(u)).unwrap_or_else(|| "-".to_string());
            let topic_display = truncate(&r.topic, 30);

            lines.push(Line::from(vec![
                Span::styled(" │  ".to_string(), Style::default().fg(app.theme.border)),
                Span::raw(format!(
                    "{:<30} {:>8} {:>10} {:>8} ",
                    topic_display,
                    format_count(r.read),
                    pending,
                    unread
                )),
                Span::styled(format!("{:^6}", r.status.symbol()), status_style),
                Span::styled(" │".to_string(), Style::default().fg(app.theme.border)),
            ]));
        }

        lines.push(Line::from(vec![Span::styled(
            format!(" ╰{:─<width$}╯", "", width = inner_width - 2),
            Style::default().fg(app.theme.border),
        )]));

        lines.push(Line::from(""));
    }

    // Writes section with border
    if !module.writes.is_empty() {
        let writes_title = format!(" Writes ({}) ", module.writes.len());
        let title_padding = inner_width.saturating_sub(writes_title.len() + 2);

        lines.push(Line::from(vec![
            Span::styled(" ╭".to_string(), Style::default().fg(app.theme.border)),
            Span::styled(writes_title, app.theme.header),
            Span::styled(
                format!("{:─<width$}╮", "", width = title_padding),
                Style::default().fg(app.theme.border),
            ),
        ]));

        // Header row
        lines.push(Line::from(vec![
            Span::styled(" │  ".to_string(), Style::default().fg(app.theme.border)),
            Span::styled(
                format!(
                    "{:<30} {:>10} {:>12} {:>6}",
                    "Topic", "Written", "Pending", "Status"
                ),
                Style::default().add_modifier(Modifier::DIM),
            ),
            Span::styled(" │".to_string(), Style::default().fg(app.theme.border)),
        ]));

        // Separator
        lines.push(Line::from(vec![Span::styled(
            format!(" ├{:─<width$}┤", "", width = inner_width - 2),
            Style::default().fg(app.theme.border),
        )]));

        for w in &module.writes {
            let status_style = app.theme.status_style(w.status);
            let pending = w.pending_for.map(format_duration).unwrap_or_else(|| "-".to_string());
            let topic_display = truncate(&w.topic, 30);

            lines.push(Line::from(vec![
                Span::styled(" │  ".to_string(), Style::default().fg(app.theme.border)),
                Span::raw(format!(
                    "{:<30} {:>10} {:>12} ",
                    topic_display,
                    format_count(w.written),
                    pending
                )),
                Span::styled(format!("{:^6}", w.status.symbol()), status_style),
                Span::styled(" │".to_string(), Style::default().fg(app.theme.border)),
            ]));
        }

        lines.push(Line::from(vec![Span::styled(
            format!(" ╰{:─<width$}╯", "", width = inner_width - 2),
            Style::default().fg(app.theme.border),
        )]));
    }

    // Footer hint
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "                    Press Esc to close".to_string(),
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
