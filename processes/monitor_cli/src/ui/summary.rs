use std::time::Duration;

use ratatui::{
    layout::{Constraint, Rect},
    style::Style,
    text::Span,
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame,
};

use crate::app::App;
use crate::data::duration::format_duration;
use crate::data::ModuleData;

/// Sparkline characters (8 levels)
const SPARKLINE_CHARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Column to sort by
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
    #[default]
    Name,
    Reads,
    Writes,
    Pending,
    Status,
}

impl SortColumn {
    pub fn next(self) -> Self {
        match self {
            SortColumn::Name => SortColumn::Reads,
            SortColumn::Reads => SortColumn::Writes,
            SortColumn::Writes => SortColumn::Pending,
            SortColumn::Pending => SortColumn::Status,
            SortColumn::Status => SortColumn::Name,
        }
    }
}

/// Render the summary view - table of all modules
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    // Get sorted module indices
    let mut modules: Vec<(usize, &ModuleData)> = data.modules.iter().enumerate().collect();
    sort_modules(&mut modules, app.sort_column, app.sort_ascending);

    let header = Row::new(vec![
        Cell::from(format_header("Module", SortColumn::Name, app)),
        Cell::from(format_header("Reads", SortColumn::Reads, app)),
        Cell::from(format_header("Rate", SortColumn::Reads, app)), // Rate uses same sort as Reads
        Cell::from(format_header("Writes", SortColumn::Writes, app)),
        Cell::from(format_header("Pending", SortColumn::Pending, app)),
        Cell::from(format_header("Unread", SortColumn::Status, app)),
        Cell::from(format_header("Trend", SortColumn::Status, app)),
        Cell::from(format_header("Status", SortColumn::Status, app)),
    ])
    .height(1)
    .style(app.theme.header);

    let rows: Vec<Row> = modules
        .iter()
        .map(|(_, m)| {
            let status_style = app.theme.status_style(m.health);

            // Calculate max pending across all topics
            let max_pending = get_max_pending(m);
            let pending_style = if let Some(d) = max_pending {
                if d >= app.thresholds.pending_critical {
                    app.theme.status_style(crate::data::HealthStatus::Critical)
                } else if d >= app.thresholds.pending_warning {
                    app.theme.status_style(crate::data::HealthStatus::Warning)
                } else {
                    Style::default()
                }
            } else {
                Style::default()
            };

            // Get total unread
            let total_unread: u64 = m.reads.iter().filter_map(|r| r.unread).sum();
            let unread_style = if total_unread >= app.thresholds.unread_critical {
                app.theme.status_style(crate::data::HealthStatus::Critical)
            } else if total_unread >= app.thresholds.unread_warning {
                app.theme.status_style(crate::data::HealthStatus::Warning)
            } else {
                Style::default()
            };

            // Get sparkline for reads
            let sparkline = render_sparkline(&app.history.get_reads_sparkline(&m.name));

            // Get read rate
            let rate = app
                .history
                .get_read_rate(&m.name)
                .map(|r| format!("{:.0}/s", r))
                .unwrap_or_else(|| "-".to_string());

            Row::new(vec![
                Cell::from(m.name.clone()),
                Cell::from(format_count(m.total_read)),
                Cell::from(rate),
                Cell::from(format_count(m.total_written)),
                Cell::from(max_pending.map(format_duration).unwrap_or_else(|| "-".to_string()))
                    .style(pending_style),
                Cell::from(if total_unread > 0 {
                    format_count(total_unread)
                } else {
                    "-".to_string()
                })
                .style(unread_style),
                Cell::from(sparkline),
                Cell::from(m.health.symbol()).style(status_style),
            ])
        })
        .collect();

    let widths = [
        Constraint::Min(25),    // Module
        Constraint::Length(10), // Reads
        Constraint::Length(8),  // Rate
        Constraint::Length(10), // Writes
        Constraint::Length(12), // Pending
        Constraint::Length(8),  // Unread
        Constraint::Length(12), // Trend/Sparkline
        Constraint::Length(6),  // Status
    ];

    let sort_indicator = match app.sort_column {
        SortColumn::Name => "name",
        SortColumn::Reads => "reads",
        SortColumn::Writes => "writes",
        SortColumn::Pending => "pending",
        SortColumn::Status => "status",
    };
    let sort_dir = if app.sort_ascending { "↑" } else { "↓" };

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(format!(
                    " Modules Summary ({}) [s:sort {}{}] ",
                    data.modules.len(),
                    sort_indicator,
                    sort_dir
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .row_highlight_style(app.theme.selected)
        .highlight_symbol("▶ ");

    // Find the visual index of the selected module
    let selected_visual_index =
        modules.iter().position(|(idx, _)| *idx == app.selected_module_index).unwrap_or(0);

    let mut state = TableState::default();
    state.select(Some(selected_visual_index));

    frame.render_stateful_widget(table, area, &mut state);
}

fn format_header(name: &str, col: SortColumn, app: &App) -> Span<'static> {
    if app.sort_column == col {
        let arrow = if app.sort_ascending { "↑" } else { "↓" };
        Span::raw(format!("{}{}", name, arrow))
    } else {
        Span::raw(name.to_string())
    }
}

fn sort_modules(modules: &mut [(usize, &ModuleData)], column: SortColumn, ascending: bool) {
    modules.sort_by(|a, b| {
        let cmp = match column {
            SortColumn::Name => a.1.name.cmp(&b.1.name),
            SortColumn::Reads => a.1.total_read.cmp(&b.1.total_read),
            SortColumn::Writes => a.1.total_written.cmp(&b.1.total_written),
            SortColumn::Pending => {
                let a_pending = get_max_pending(a.1).unwrap_or(Duration::ZERO);
                let b_pending = get_max_pending(b.1).unwrap_or(Duration::ZERO);
                a_pending.cmp(&b_pending)
            }
            SortColumn::Status => a.1.health.cmp(&b.1.health),
        };
        if ascending {
            cmp
        } else {
            cmp.reverse()
        }
    });
}

fn get_max_pending(module: &ModuleData) -> Option<Duration> {
    module
        .reads
        .iter()
        .filter_map(|r| r.pending_for)
        .chain(module.writes.iter().filter_map(|w| w.pending_for))
        .max()
}

fn render_sparkline(data: &[u8]) -> String {
    if data.is_empty() {
        return "        ".to_string(); // 8 spaces placeholder
    }

    // Take last 8 values
    let values: Vec<u8> = data.iter().rev().take(8).rev().copied().collect();

    values.iter().map(|&v| SPARKLINE_CHARS[v.min(7) as usize]).collect()
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
