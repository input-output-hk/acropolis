use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::data::duration::format_duration;

/// Render the module detail as a modal overlay
pub fn render_overlay(frame: &mut Frame, app: &App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let Some(module) = data.modules.get(app.selected_module_index) else {
        return;
    };

    // Calculate overlay size (80% of screen, max dimensions)
    let overlay_width = (area.width * 85 / 100).min(120).max(60);
    let overlay_height = (area.height * 85 / 100).min(40).max(15);
    let x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
    let y = area.y + (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    // Clear the area behind the overlay
    frame.render_widget(Clear, overlay_area);

    // Build content lines
    let mut lines: Vec<Line> = Vec::new();

    // Module header
    lines.push(Line::from(vec![Span::styled(
        format!(" {} ", module.name),
        Style::default().add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![
        Span::styled("  Reads: ", Style::default().add_modifier(Modifier::DIM)),
        Span::raw(format!("{}", module.total_read)),
        Span::styled("  Writes: ", Style::default().add_modifier(Modifier::DIM)),
        Span::raw(format!("{}", module.total_written)),
        Span::styled("  Status: ", Style::default().add_modifier(Modifier::DIM)),
        Span::styled(
            module.health.symbol(),
            app.theme.status_style(module.health),
        ),
    ]));
    lines.push(Line::from(""));

    // Reads section
    if !module.reads.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!(" Reads ({}) ", module.reads.len()),
            app.theme.header,
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!(
                "  {:<35} {:>10} {:>12} {:>8} {:>6}",
                "Topic", "Read", "Pending", "Unread", "Status"
            ),
            Style::default().add_modifier(Modifier::DIM),
        )]));

        for r in &module.reads {
            let status_style = app.theme.status_style(r.status);
            let pending = r.pending_for.map(format_duration).unwrap_or_else(|| "-".to_string());
            let unread = r.unread.map(|u| u.to_string()).unwrap_or_else(|| "-".to_string());
            let topic_display = truncate(&r.topic, 35);

            lines.push(Line::from(vec![
                Span::raw(format!(
                    "  {:<35} {:>10} {:>12} {:>8} ",
                    topic_display, r.read, pending, unread
                )),
                Span::styled(format!("{:>6}", r.status.symbol()), status_style),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Writes section
    if !module.writes.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!(" Writes ({}) ", module.writes.len()),
            app.theme.header,
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!(
                "  {:<35} {:>10} {:>12} {:>6}",
                "Topic", "Written", "Pending", "Status"
            ),
            Style::default().add_modifier(Modifier::DIM),
        )]));

        for w in &module.writes {
            let status_style = app.theme.status_style(w.status);
            let pending = w.pending_for.map(format_duration).unwrap_or_else(|| "-".to_string());
            let topic_display = truncate(&w.topic, 35);

            lines.push(Line::from(vec![
                Span::raw(format!(
                    "  {:<35} {:>10} {:>12} ",
                    topic_display, w.written, pending
                )),
                Span::styled(format!("{:>6}", w.status.symbol()), status_style),
            ]));
        }
    }

    // Footer hint
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        " Press Esc or Enter to close ",
        Style::default().add_modifier(Modifier::DIM),
    )]));

    let block = Block::default()
        .title(format!(" Module Detail "))
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
