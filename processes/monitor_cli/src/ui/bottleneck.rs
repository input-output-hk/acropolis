use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::app::App;
use crate::data::{duration::format_duration, HealthStatus, UnhealthyTopic};
use std::time::Duration;

/// Column to sort bottlenecks by
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BottleneckSortColumn {
    #[default]
    Status,
    Module,
    Topic,
    Kind,
    Pending,
    Unread,
}

impl BottleneckSortColumn {
    pub fn next(self) -> Self {
        match self {
            Self::Status => Self::Module,
            Self::Module => Self::Topic,
            Self::Topic => Self::Kind,
            Self::Kind => Self::Pending,
            Self::Pending => Self::Unread,
            Self::Unread => Self::Status,
        }
    }
}

/// Render the bottleneck view as a table (like Summary view)
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let all_unhealthy = data.unhealthy_topics();

    // Filter by search text
    let filtered: Vec<_> = all_unhealthy
        .iter()
        .filter(|(module, topic)| {
            if app.filter_text.is_empty() {
                return true;
            }
            let search = app.filter_text.to_lowercase();
            module.name.to_lowercase().contains(&search)
                || topic.topic().to_lowercase().contains(&search)
        })
        .collect();

    if filtered.is_empty() && all_unhealthy.is_empty() {
        render_healthy_message(frame, app, area);
        return;
    }

    // Sort the filtered results
    let mut sorted: Vec<_> = filtered.into_iter().collect();
    sort_bottlenecks(
        &mut sorted,
        app.bottleneck_sort_column,
        app.bottleneck_sort_ascending,
    );

    // Count by severity
    let critical_count =
        sorted.iter().filter(|(_, t)| t.status() == HealthStatus::Critical).count();
    let warning_count = sorted.iter().filter(|(_, t)| t.status() == HealthStatus::Warning).count();

    // Build header row with sort indicators
    let header = Row::new(vec![
        Cell::from(format_header("Status", BottleneckSortColumn::Status, app)),
        Cell::from(format_header("Module", BottleneckSortColumn::Module, app)),
        Cell::from(format_header("Topic", BottleneckSortColumn::Topic, app)),
        Cell::from(format_header("Kind", BottleneckSortColumn::Kind, app)),
        Cell::from(format_header("Pending", BottleneckSortColumn::Pending, app)),
        Cell::from(format_header("Unread", BottleneckSortColumn::Unread, app)),
    ])
    .height(1)
    .style(app.theme.header);

    // Build data rows
    let rows: Vec<Row> = sorted
        .iter()
        .map(|(module, topic)| {
            let status_style = app.theme.status_style(topic.status());
            let status_label = match topic.status() {
                HealthStatus::Critical => "CRIT",
                HealthStatus::Warning => "WARN",
                HealthStatus::Healthy => "OK",
            };

            let pending_info =
                topic.pending_for().map(format_duration).unwrap_or_else(|| "-".to_string());

            let unread_info = if let UnhealthyTopic::Read(r) = topic {
                r.unread
                    .filter(|&u| u > 0)
                    .map(|u| format!("{}", u))
                    .unwrap_or_else(|| "-".to_string())
            } else {
                "-".to_string()
            };

            let kind_label = match topic {
                UnhealthyTopic::Read(_) => "R",
                UnhealthyTopic::Write(_) => "W",
            };

            Row::new(vec![
                Cell::from(status_label).style(status_style),
                Cell::from(module.name.clone())
                    .style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from(topic.topic().to_string()),
                Cell::from(kind_label).style(Style::default().add_modifier(Modifier::DIM)),
                Cell::from(pending_info).style(status_style),
                Cell::from(unread_info).style(status_style),
            ])
        })
        .collect();

    // Use Fill constraints like Summary view for even distribution
    let widths = [
        Constraint::Length(6),  // Status - fixed
        Constraint::Fill(2),    // Module - 2x share
        Constraint::Fill(3),    // Topic - 3x share (usually longer)
        Constraint::Length(4),  // Kind - fixed
        Constraint::Length(10), // Pending - fixed
        Constraint::Length(8),  // Unread - fixed
    ];

    // Build title
    let sort_indicator = match app.bottleneck_sort_column {
        BottleneckSortColumn::Status => "status",
        BottleneckSortColumn::Module => "module",
        BottleneckSortColumn::Topic => "topic",
        BottleneckSortColumn::Kind => "kind",
        BottleneckSortColumn::Pending => "pending",
        BottleneckSortColumn::Unread => "unread",
    };
    let sort_dir = if app.bottleneck_sort_ascending {
        "↑"
    } else {
        "↓"
    };

    let filter_info = if app.filter_active {
        format!(" /{}_", app.filter_text)
    } else if !app.filter_text.is_empty() {
        format!(" /{}/ [c:clear]", app.filter_text)
    } else {
        String::new()
    };

    let position_info = if !sorted.is_empty() {
        format!(" [{}/{}]", app.selected_topic_index + 1, sorted.len())
    } else {
        String::new()
    };

    let title = format!(
        " Bottlenecks ({} crit, {} warn) [s:sort {}{}]{}{} ",
        critical_count, warning_count, sort_indicator, sort_dir, filter_info, position_info
    );

    let border_color = if critical_count > 0 {
        app.theme.critical
    } else if warning_count > 0 {
        app.theme.warning
    } else {
        app.theme.border
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(app.theme.border_type)
                .border_style(Style::default().fg(border_color)),
        )
        .row_highlight_style(app.theme.selected)
        .highlight_symbol("▶ ");

    let mut state = TableState::default();
    state.select(Some(
        app.selected_topic_index.min(sorted.len().saturating_sub(1)),
    ));

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_healthy_message(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Bottlenecks ")
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.healthy));

    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled("    ✓ ", Style::default().fg(app.theme.healthy)),
            Span::styled(
                "All systems healthy!",
                Style::default().fg(app.theme.healthy).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "      No modules reporting warnings or critical issues.",
            Style::default().add_modifier(Modifier::DIM),
        )]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn format_header(name: &str, col: BottleneckSortColumn, app: &App) -> Span<'static> {
    if app.bottleneck_sort_column == col {
        let arrow = if app.bottleneck_sort_ascending {
            "↑"
        } else {
            "↓"
        };
        Span::raw(format!("{}{}", name, arrow))
    } else {
        Span::raw(name.to_string())
    }
}

fn sort_bottlenecks(
    items: &mut [&(&crate::data::ModuleData, UnhealthyTopic)],
    column: BottleneckSortColumn,
    ascending: bool,
) {
    items.sort_by(|a, b| {
        let cmp = match column {
            BottleneckSortColumn::Status => a.1.status().cmp(&b.1.status()),
            BottleneckSortColumn::Module => a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()),
            BottleneckSortColumn::Topic => {
                a.1.topic().to_lowercase().cmp(&b.1.topic().to_lowercase())
            }
            BottleneckSortColumn::Kind => {
                let a_kind = matches!(a.1, UnhealthyTopic::Write(_));
                let b_kind = matches!(b.1, UnhealthyTopic::Write(_));
                a_kind.cmp(&b_kind)
            }
            BottleneckSortColumn::Pending => {
                let a_pending = a.1.pending_for().unwrap_or(Duration::ZERO);
                let b_pending = b.1.pending_for().unwrap_or(Duration::ZERO);
                a_pending.cmp(&b_pending)
            }
            BottleneckSortColumn::Unread => {
                let a_unread = get_unread(&a.1);
                let b_unread = get_unread(&b.1);
                a_unread.cmp(&b_unread)
            }
        };
        if ascending {
            cmp
        } else {
            cmp.reverse()
        }
    });
}

fn get_unread(topic: &UnhealthyTopic) -> u64 {
    match topic {
        UnhealthyTopic::Read(r) => r.unread.unwrap_or(0),
        UnhealthyTopic::Write(_) => 0,
    }
}
