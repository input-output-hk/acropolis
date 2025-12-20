use ratatui::{
    layout::{Constraint, Rect},
    style::Style,
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame,
};

use crate::app::App;

/// Render the summary view - table of all modules
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let header = Row::new(vec![
        Cell::from("Module").style(app.theme.header),
        Cell::from("Reads").style(app.theme.header),
        Cell::from("Writes").style(app.theme.header),
        Cell::from("Status").style(app.theme.header),
    ])
    .height(1);

    let rows: Vec<Row> = data
        .modules
        .iter()
        .map(|m| {
            let status_style = app.theme.status_style(m.health);
            Row::new(vec![
                Cell::from(m.name.clone()),
                Cell::from(format_count(m.total_read)),
                Cell::from(format_count(m.total_written)),
                Cell::from(m.health.symbol()).style(status_style),
            ])
        })
        .collect();

    let widths = [
        Constraint::Min(30),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(" Modules Summary ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .row_highlight_style(app.theme.selected)
        .highlight_symbol("> ");

    let mut state = TableState::default();
    state.select(Some(app.selected_module_index));

    frame.render_stateful_widget(table, area, &mut state);
}

/// Format large numbers with K/M suffixes
fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
