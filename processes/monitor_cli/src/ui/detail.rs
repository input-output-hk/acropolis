use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
    Frame,
};

use crate::app::App;
use crate::data::duration::format_duration;

/// Minimum dimensions required for the detail overlay to render properly.
const MIN_OVERLAY_WIDTH: u16 = 50;
const MIN_OVERLAY_HEIGHT: u16 = 16;

/// Render the module detail as a modal overlay.
///
/// The overlay displays detailed information about the selected module including
/// its health status, read/write counts, and per-topic statistics.
pub fn render_overlay(frame: &mut Frame, app: &App, area: Rect) {
    // Skip rendering if terminal is too small for the overlay
    if area.width < MIN_OVERLAY_WIDTH || area.height < MIN_OVERLAY_HEIGHT {
        return;
    }

    let Some(ref data) = app.data else {
        return;
    };

    // Get the actual module from the visual index
    let Some(raw_index) = app.get_selected_module_raw_index() else {
        return;
    };
    let Some(module) = data.modules.get(raw_index) else {
        return;
    };

    // Calculate overlay size - use most of the screen
    // Width: 95% of screen, max 100 chars
    let overlay_width = (area.width * 95 / 100).min(100).max(MIN_OVERLAY_WIDTH);
    // Height: 90% of screen, max 50 lines
    let overlay_height = (area.height * 90 / 100).min(50).max(MIN_OVERLAY_HEIGHT);

    let x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
    let y = area.y + (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    // Clear the area behind the overlay
    frame.render_widget(Clear, overlay_area);

    // Split overlay into header and content sections
    let chunks = Layout::vertical([
        Constraint::Length(5), // Header with module info
        Constraint::Min(10),   // Content (reads/writes tables)
        Constraint::Length(1), // Footer
    ])
    .split(overlay_area);

    // ===== HEADER SECTION =====
    let health_style = app.theme.status_style(module.health);
    let health_label = match module.health {
        crate::data::HealthStatus::Healthy => "Healthy",
        crate::data::HealthStatus::Warning => "Warning",
        crate::data::HealthStatus::Critical => "Critical",
    };

    let header_lines = vec![
        Line::from(vec![Span::styled(
            format!(" {} ", module.name),
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::raw(" Total Read: "),
            Span::styled(
                format_count(module.total_read),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("    Total Written: "),
            Span::styled(
                format_count(module.total_written),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("    Status: "),
            Span::styled(
                format!("{} {}", module.health.symbol(), health_label),
                health_style.add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let header_block = Block::default()
        .title(" Module Detail ")
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.highlight));

    let header = Paragraph::new(header_lines).block(header_block);
    frame.render_widget(header, chunks[0]);

    // ===== CONTENT SECTION (Reads and Writes) =====
    // Split content area for reads and writes
    let content_chunks = Layout::vertical([
        Constraint::Percentage(50), // Reads
        Constraint::Percentage(50), // Writes
    ])
    .split(chunks[1]);

    // ----- READS TABLE -----
    if !module.reads.is_empty() {
        let reads_header = Row::new(vec![
            Cell::from("Topic"),
            Cell::from("Read"),
            Cell::from("Pending"),
            Cell::from("Unread"),
            Cell::from("Status"),
        ])
        .height(1)
        .style(app.theme.header);

        let reads_rows: Vec<Row> = module
            .reads
            .iter()
            .map(|r| {
                let status_style = app.theme.status_style(r.status);
                Row::new(vec![
                    Cell::from(r.topic.clone()),
                    Cell::from(format_count(r.read)),
                    Cell::from(r.pending_for.map(format_duration).unwrap_or("-".into())),
                    Cell::from(r.unread.map(format_count).unwrap_or("-".into())),
                    Cell::from(r.status.symbol()).style(status_style),
                ])
            })
            .collect();

        let reads_widths = [
            Constraint::Fill(3),    // Topic
            Constraint::Length(10), // Read
            Constraint::Length(12), // Pending
            Constraint::Length(10), // Unread
            Constraint::Length(8),  // Status
        ];

        let reads_table = Table::new(reads_rows, reads_widths).header(reads_header).block(
            Block::default()
                .title(format!(" Reads ({}) ", module.reads.len()))
                .borders(Borders::ALL)
                .border_type(app.theme.border_type)
                .border_style(Style::default().fg(app.theme.border)),
        );

        frame.render_widget(reads_table, content_chunks[0]);
    } else {
        let empty_block = Block::default()
            .title(" Reads (0) ")
            .borders(Borders::ALL)
            .border_type(app.theme.border_type)
            .border_style(Style::default().fg(app.theme.border));
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No read topics",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .block(empty_block);
        frame.render_widget(empty, content_chunks[0]);
    }

    // ----- WRITES TABLE -----
    if !module.writes.is_empty() {
        let writes_header = Row::new(vec![
            Cell::from("Topic"),
            Cell::from("Written"),
            Cell::from("Pending"),
            Cell::from("Status"),
        ])
        .height(1)
        .style(app.theme.header);

        let writes_rows: Vec<Row> = module
            .writes
            .iter()
            .map(|w| {
                let status_style = app.theme.status_style(w.status);
                Row::new(vec![
                    Cell::from(w.topic.clone()),
                    Cell::from(format_count(w.written)),
                    Cell::from(w.pending_for.map(format_duration).unwrap_or("-".into())),
                    Cell::from(w.status.symbol()).style(status_style),
                ])
            })
            .collect();

        let writes_widths = [
            Constraint::Fill(3),    // Topic
            Constraint::Length(10), // Written
            Constraint::Length(12), // Pending
            Constraint::Length(8),  // Status
        ];

        let writes_table = Table::new(writes_rows, writes_widths).header(writes_header).block(
            Block::default()
                .title(format!(" Writes ({}) ", module.writes.len()))
                .borders(Borders::ALL)
                .border_type(app.theme.border_type)
                .border_style(Style::default().fg(app.theme.border)),
        );

        frame.render_widget(writes_table, content_chunks[1]);
    } else {
        let empty_block = Block::default()
            .title(" Writes (0) ")
            .borders(Borders::ALL)
            .border_type(app.theme.border_type)
            .border_style(Style::default().fg(app.theme.border));
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No write topics",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .block(empty_block);
        frame.render_widget(empty, content_chunks[1]);
    }

    // ===== FOOTER =====
    let footer = Paragraph::new(Line::from(vec![Span::styled(
        " Press Esc to close ",
        Style::default().add_modifier(Modifier::DIM),
    )]));
    frame.render_widget(footer, chunks[2]);
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
