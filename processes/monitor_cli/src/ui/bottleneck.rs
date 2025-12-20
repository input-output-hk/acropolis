use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
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

// Fixed column widths
const COL_STATUS: usize = 8;
const COL_MODULE: usize = 20;
const COL_TOPIC: usize = 28;
const COL_KIND: usize = 4;
const COL_PENDING: usize = 10;
const COL_UNREAD: usize = 8;

/// Render the bottleneck view - list of unhealthy topics with headers, search, and sorting
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

    // Count by severity (from sorted results)
    let critical_count =
        sorted.iter().filter(|(_, t)| t.status() == HealthStatus::Critical).count();
    let warning_count = sorted.iter().filter(|(_, t)| t.status() == HealthStatus::Warning).count();

    let mut lines: Vec<Line> = Vec::new();

    // Header row
    lines.push(render_header(app));

    // Separator
    let sep_width = COL_STATUS + COL_MODULE + COL_TOPIC + COL_KIND + COL_PENDING + COL_UNREAD + 18; // 18 for separators and padding
    lines.push(Line::from(vec![Span::styled(
        format!(" {:─<w$}", "", w = sep_width),
        Style::default().fg(app.theme.border),
    )]));

    // Data rows
    for (idx, (module, topic)) in sorted.iter().enumerate() {
        let is_selected = idx == app.selected_topic_index;
        lines.push(render_topic_item(app, module, topic, is_selected));
    }

    // Empty state when filter has no matches
    if sorted.is_empty() && !app.filter_text.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            format!("   No matches for \"{}\"", app.filter_text),
            Style::default().add_modifier(Modifier::DIM),
        )]));
    }

    // Build title
    let title = if !app.filter_text.is_empty() {
        format!(
            " Bottlenecks [filter: \"{}\"] ({}/{} shown) ",
            app.filter_text,
            sorted.len(),
            all_unhealthy.len()
        )
    } else {
        format!(
            " Bottlenecks ({} critical, {} warning) ",
            critical_count, warning_count
        )
    };

    let border_color = if critical_count > 0 {
        app.theme.critical
    } else if warning_count > 0 {
        app.theme.warning
    } else {
        app.theme.border
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(border_color));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_healthy_message(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Bottlenecks ")
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.border));

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(app.theme.healthy)),
            Span::styled(
                "All systems healthy!",
                Style::default().fg(app.theme.healthy).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "    No modules reporting warnings or critical issues.",
            Style::default().add_modifier(Modifier::DIM),
        )]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_header(app: &App) -> Line<'static> {
    let col = app.bottleneck_sort_column;
    let asc = app.bottleneck_sort_ascending;

    let header_style = app.theme.header;

    let fmt_header = |name: &str,
                      width: usize,
                      sort_col: BottleneckSortColumn,
                      right_align: bool|
     -> Span<'static> {
        let arrow = if col == sort_col {
            if asc {
                "↑"
            } else {
                "↓"
            }
        } else {
            " "
        };
        let style = if col == sort_col {
            header_style.add_modifier(Modifier::BOLD)
        } else {
            header_style
        };
        let text = if right_align {
            format!("{:>w$}{}", name, arrow, w = width - 1)
        } else {
            format!("{:<w$}{}", name, arrow, w = width - 1)
        };
        Span::styled(text, style)
    };

    let sep = Span::styled("│", Style::default().fg(app.theme.border));

    Line::from(vec![
        Span::raw("   "), // Selection indicator space
        fmt_header("Status", COL_STATUS, BottleneckSortColumn::Status, false),
        sep.clone(),
        fmt_header("Module", COL_MODULE, BottleneckSortColumn::Module, false),
        sep.clone(),
        fmt_header("Topic", COL_TOPIC, BottleneckSortColumn::Topic, false),
        sep.clone(),
        fmt_header("Kind", COL_KIND, BottleneckSortColumn::Kind, false),
        sep.clone(),
        fmt_header("Pending", COL_PENDING, BottleneckSortColumn::Pending, true),
        sep.clone(),
        fmt_header("Unread", COL_UNREAD, BottleneckSortColumn::Unread, true),
    ])
}

fn render_topic_item<'a>(
    app: &App,
    module: &'a crate::data::ModuleData,
    topic: &'a UnhealthyTopic,
    is_selected: bool,
) -> Line<'a> {
    let status_style = app.theme.status_style(topic.status());
    let status_label = match topic.status() {
        HealthStatus::Critical => "CRIT",
        HealthStatus::Warning => "WARN",
        HealthStatus::Healthy => "OK",
    };

    let pending_info =
        topic.pending_for().map(|d| format_duration(d)).unwrap_or_else(|| "-".to_string());

    let unread_info = if let UnhealthyTopic::Read(r) = topic {
        r.unread.filter(|&u| u > 0).map(|u| format!("{}", u)).unwrap_or_else(|| "-".to_string())
    } else {
        "-".to_string()
    };

    let kind_label = match topic {
        UnhealthyTopic::Read(_) => "R",
        UnhealthyTopic::Write(_) => "W",
    };

    let sep = Span::styled("│", Style::default().fg(app.theme.border));

    let row_style = if is_selected {
        app.theme.selected
    } else {
        Style::default()
    };

    let selector = if is_selected { " ▶ " } else { "   " };

    Line::from(vec![
        Span::styled(selector, row_style),
        Span::styled(
            format!("{:<w$}", status_label, w = COL_STATUS),
            status_style,
        ),
        sep.clone(),
        Span::styled(
            format!(
                "{:<w$}",
                truncate(&module.name, COL_MODULE - 1),
                w = COL_MODULE
            ),
            row_style.add_modifier(Modifier::BOLD),
        ),
        sep.clone(),
        Span::styled(
            format!(
                "{:<w$}",
                truncate(topic.topic(), COL_TOPIC - 1),
                w = COL_TOPIC
            ),
            row_style,
        ),
        sep.clone(),
        Span::styled(
            format!("{:<w$}", kind_label, w = COL_KIND),
            row_style.add_modifier(Modifier::DIM),
        ),
        sep.clone(),
        Span::styled(
            format!("{:>w$}", pending_info, w = COL_PENDING),
            status_style,
        ),
        sep.clone(),
        Span::styled(format!("{:>w$}", unread_info, w = COL_UNREAD), status_style),
    ])
}

fn sort_bottlenecks(
    items: &mut [&(&crate::data::ModuleData, UnhealthyTopic)],
    column: BottleneckSortColumn,
    ascending: bool,
) {
    items.sort_by(|a, b| {
        let cmp = match column {
            BottleneckSortColumn::Status => {
                // Critical > Warning > Healthy (so reverse the natural order for descending = critical first)
                a.1.status().cmp(&b.1.status())
            }
            BottleneckSortColumn::Module => a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()),
            BottleneckSortColumn::Topic => {
                a.1.topic().to_lowercase().cmp(&b.1.topic().to_lowercase())
            }
            BottleneckSortColumn::Kind => {
                let a_kind = matches!(a.1, UnhealthyTopic::Write(_)); // W before R
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

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}
