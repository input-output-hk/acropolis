use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
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

    let mut items: Vec<ListItem> = Vec::new();

    // Column header row
    items.push(render_header(app));
    items.push(ListItem::new(Line::from(vec![Span::styled(
        "─".repeat(100),
        Style::default().fg(app.theme.border),
    )])));

    // Data rows
    for (module, topic) in &sorted {
        items.push(render_topic_item(app, module, topic));
    }

    // Build title
    let mut title = format!(
        " Bottlenecks ({} critical, {} warning) ",
        critical_count, warning_count
    );
    if !app.filter_text.is_empty() {
        title = format!(
            " Bottlenecks [filter: \"{}\"] ({}/{} shown) ",
            app.filter_text,
            sorted.len(),
            all_unhealthy.len()
        );
    }

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

    let list =
        List::new(items).block(block).highlight_style(app.theme.selected).highlight_symbol("▶ ");

    // Selection state (offset by 2 for header + separator)
    let selectable_count = sorted.len();
    let mut state = ListState::default();
    if selectable_count > 0 {
        let selected_idx = app.selected_topic_index.min(selectable_count.saturating_sub(1));
        state.select(Some(selected_idx + 2)); // +2 for header and separator
    }

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_healthy_message(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Bottlenecks ")
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.border));

    let items = vec![
        ListItem::new(Line::from("")),
        ListItem::new(Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(app.theme.healthy)),
            Span::styled(
                "All systems healthy!",
                Style::default().fg(app.theme.healthy).add_modifier(Modifier::BOLD),
            ),
        ])),
        ListItem::new(Line::from("")),
        ListItem::new(Line::from(vec![Span::styled(
            "    No modules reporting warnings or critical issues.",
            Style::default().add_modifier(Modifier::DIM),
        )])),
    ];

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_header(app: &App) -> ListItem<'static> {
    let col = app.bottleneck_sort_column;
    let asc = app.bottleneck_sort_ascending;

    let header_style = app.theme.header;
    let sort_arrow = |c: BottleneckSortColumn| -> &'static str {
        if col == c {
            if asc {
                "↑"
            } else {
                "↓"
            }
        } else {
            ""
        }
    };

    let line = Line::from(vec![
        Span::raw("    "), // Space for selection indicator
        Span::styled(
            format!(
                "{:<8}{}",
                "Status",
                sort_arrow(BottleneckSortColumn::Status)
            ),
            if col == BottleneckSortColumn::Status {
                header_style.add_modifier(Modifier::BOLD)
            } else {
                header_style
            },
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(
            format!(
                "{:<24}{}",
                "Module",
                sort_arrow(BottleneckSortColumn::Module)
            ),
            if col == BottleneckSortColumn::Module {
                header_style.add_modifier(Modifier::BOLD)
            } else {
                header_style
            },
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(
            format!("{:<30}{}", "Topic", sort_arrow(BottleneckSortColumn::Topic)),
            if col == BottleneckSortColumn::Topic {
                header_style.add_modifier(Modifier::BOLD)
            } else {
                header_style
            },
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(
            format!("{:<4}{}", "Kind", sort_arrow(BottleneckSortColumn::Kind)),
            if col == BottleneckSortColumn::Kind {
                header_style.add_modifier(Modifier::BOLD)
            } else {
                header_style
            },
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(
            format!(
                "{:>12}{}",
                "Pending",
                sort_arrow(BottleneckSortColumn::Pending)
            ),
            if col == BottleneckSortColumn::Pending {
                header_style.add_modifier(Modifier::BOLD)
            } else {
                header_style
            },
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(
            format!(
                "{:>8}{}",
                "Unread",
                sort_arrow(BottleneckSortColumn::Unread)
            ),
            if col == BottleneckSortColumn::Unread {
                header_style.add_modifier(Modifier::BOLD)
            } else {
                header_style
            },
        ),
    ]);

    ListItem::new(line)
}

fn render_topic_item<'a>(
    app: &App,
    module: &'a crate::data::ModuleData,
    topic: &'a UnhealthyTopic,
) -> ListItem<'a> {
    let status_style = app.theme.status_style(topic.status());
    let status_label = match topic.status() {
        HealthStatus::Critical => "CRITICAL",
        HealthStatus::Warning => "WARNING",
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

    let line = Line::from(vec![
        Span::styled(format!("{:<8}", status_label), status_style),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(
            format!("{:<24}", truncate(&module.name, 24)),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(
            format!("{:<30}", truncate(topic.topic(), 30)),
            Style::default(),
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(
            format!("{:<4}", kind_label),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(format!("{:>12}", pending_info), status_style),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(format!("{:>8}", unread_info), status_style),
    ]);

    ListItem::new(line)
}

fn sort_bottlenecks(
    items: &mut [&(&crate::data::ModuleData, UnhealthyTopic)],
    column: BottleneckSortColumn,
    ascending: bool,
) {
    items.sort_by(|a, b| {
        let cmp = match column {
            BottleneckSortColumn::Status => a.1.status().cmp(&b.1.status()),
            BottleneckSortColumn::Module => a.0.name.cmp(&b.0.name),
            BottleneckSortColumn::Topic => a.1.topic().cmp(b.1.topic()),
            BottleneckSortColumn::Kind => {
                let a_kind = matches!(a.1, UnhealthyTopic::Read(_));
                let b_kind = matches!(b.1, UnhealthyTopic::Read(_));
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
