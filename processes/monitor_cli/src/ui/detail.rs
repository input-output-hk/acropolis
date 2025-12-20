use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

use crate::app::App;
use crate::data::duration::format_duration;

/// Render the module detail view
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let Some(module) = data.modules.get(app.selected_module_index) else {
        return;
    };

    // Split into header, reads section, writes section
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Min(5),    // Reads
        Constraint::Min(5),    // Writes
    ])
    .split(area);

    // Header with module name
    let header = Paragraph::new(format!(
        " Module: {}  |  Total Reads: {}  |  Total Writes: {}",
        module.name, module.total_read, module.total_written
    ))
    .style(app.theme.header)
    .block(
        Block::default().borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)),
    );
    frame.render_widget(header, chunks[0]);

    // Reads table
    render_reads_table(frame, app, chunks[1], module);

    // Writes table
    render_writes_table(frame, app, chunks[2], module);
}

fn render_reads_table(frame: &mut Frame, app: &App, area: Rect, module: &crate::data::ModuleData) {
    let header = Row::new(vec![
        Cell::from("Topic").style(app.theme.header),
        Cell::from("Read").style(app.theme.header),
        Cell::from("Pending").style(app.theme.header),
        Cell::from("Unread").style(app.theme.header),
        Cell::from("Status").style(app.theme.header),
    ]);

    let rows: Vec<Row> = module
        .reads
        .iter()
        .map(|r| {
            let status_style = app.theme.status_style(r.status);
            Row::new(vec![
                Cell::from(r.topic.clone()),
                Cell::from(r.read.to_string()),
                Cell::from(r.pending_for.map(format_duration).unwrap_or_else(|| "-".to_string())),
                Cell::from(r.unread.map(|u| u.to_string()).unwrap_or_else(|| "-".to_string())),
                Cell::from(r.status.symbol()).style(status_style),
            ])
        })
        .collect();

    let widths = [
        Constraint::Min(30),
        Constraint::Length(12),
        Constraint::Length(14),
        Constraint::Length(10),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .title(format!(" Reads ({}) ", module.reads.len()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border)),
    );

    frame.render_widget(table, area);
}

fn render_writes_table(frame: &mut Frame, app: &App, area: Rect, module: &crate::data::ModuleData) {
    let header = Row::new(vec![
        Cell::from("Topic").style(app.theme.header),
        Cell::from("Written").style(app.theme.header),
        Cell::from("Pending").style(app.theme.header),
        Cell::from("Status").style(app.theme.header),
    ]);

    let rows: Vec<Row> = module
        .writes
        .iter()
        .map(|w| {
            let status_style = app.theme.status_style(w.status);
            Row::new(vec![
                Cell::from(w.topic.clone()),
                Cell::from(w.written.to_string()),
                Cell::from(w.pending_for.map(format_duration).unwrap_or_else(|| "-".to_string())),
                Cell::from(w.status.symbol()).style(status_style),
            ])
        })
        .collect();

    let widths = [
        Constraint::Min(30),
        Constraint::Length(12),
        Constraint::Length(14),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .title(format!(" Writes ({}) ", module.writes.len()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border)),
    );

    frame.render_widget(table, area);
}
