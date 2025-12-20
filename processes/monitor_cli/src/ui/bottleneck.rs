use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::app::App;
use crate::data::{duration::format_duration, HealthStatus, UnhealthyTopic};

/// Render the bottleneck view - list of unhealthy topics grouped by severity
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let unhealthy = data.unhealthy_topics();

    if unhealthy.is_empty() {
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
        return;
    }

    // Separate by severity
    let critical: Vec<_> =
        unhealthy.iter().filter(|(_, t)| t.status() == HealthStatus::Critical).collect();
    let warning: Vec<_> =
        unhealthy.iter().filter(|(_, t)| t.status() == HealthStatus::Warning).collect();

    let mut items: Vec<ListItem> = Vec::new();
    let mut item_count = 0;

    // Critical section
    if !critical.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!("━━━ CRITICAL ({}) ", critical.len()),
                Style::default().fg(app.theme.critical).add_modifier(Modifier::BOLD),
            ),
            Span::styled("━".repeat(20), Style::default().fg(app.theme.critical)),
        ])));

        for (module, topic) in &critical {
            items.push(render_topic_item(app, module, topic, item_count));
            item_count += 1;
        }
        items.push(ListItem::new(Line::from("")));
    }

    // Warning section
    if !warning.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!("━━━ WARNING ({}) ", warning.len()),
                Style::default().fg(app.theme.warning).add_modifier(Modifier::BOLD),
            ),
            Span::styled("━".repeat(21), Style::default().fg(app.theme.warning)),
        ])));

        for (module, topic) in &warning {
            items.push(render_topic_item(app, module, topic, item_count));
            item_count += 1;
        }
    }

    let block = Block::default()
        .title(format!(
            " Bottlenecks ({} critical, {} warning) ",
            critical.len(),
            warning.len()
        ))
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(if !critical.is_empty() {
            app.theme.critical
        } else {
            app.theme.warning
        }));

    let list =
        List::new(items).block(block).highlight_style(app.theme.selected).highlight_symbol("▶ ");

    // Adjust selection to skip header lines
    let selectable_count = unhealthy.len();
    let mut state = ListState::default();

    // Map visual index to actual list position (accounting for headers)
    let selected_idx = app.selected_topic_index.min(selectable_count.saturating_sub(1));
    let visual_idx = if !critical.is_empty() {
        if selected_idx < critical.len() {
            selected_idx + 1 // Skip critical header
        } else {
            selected_idx + 3 // Skip critical header, items, spacer, warning header
        }
    } else {
        selected_idx + 1 // Skip warning header
    };

    state.select(Some(visual_idx));

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_topic_item<'a>(
    app: &App,
    module: &'a crate::data::ModuleData,
    topic: &'a UnhealthyTopic,
    _index: usize,
) -> ListItem<'a> {
    let status_style = app.theme.status_style(topic.status());
    let status_icon = match topic.status() {
        HealthStatus::Critical => "●",
        HealthStatus::Warning => "○",
        HealthStatus::Healthy => "·",
    };

    let pending_info =
        topic.pending_for().map(|d| format_duration(d)).unwrap_or_else(|| "-".to_string());

    let unread_info = if let UnhealthyTopic::Read(r) = topic {
        r.unread.filter(|&u| u > 0).map(|u| format!("{}", u)).unwrap_or_else(|| "-".to_string())
    } else {
        "-".to_string()
    };

    let kind_indicator = match topic {
        UnhealthyTopic::Read(_) => "R",
        UnhealthyTopic::Write(_) => "W",
    };

    let line = Line::from(vec![
        Span::styled(format!("  {} ", status_icon), status_style),
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
            format!("[{}]", kind_indicator),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(format!("pending: {:>10}", pending_info), status_style),
        Span::styled(" │ ", Style::default().fg(app.theme.border)),
        Span::styled(format!("unread: {:>6}", unread_info), status_style),
    ]);

    ListItem::new(line)
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}
