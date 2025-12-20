use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::app::App;
use crate::data::{duration::format_duration, HealthStatus, UnhealthyTopic};

/// Render the bottleneck view - list of unhealthy topics
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let unhealthy = data.unhealthy_topics();

    if unhealthy.is_empty() {
        let block = Block::default()
            .title(" Bottlenecks ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border));

        let items = vec![ListItem::new(Line::from(vec![Span::styled(
            "  All systems healthy!",
            Style::default().fg(app.theme.healthy),
        )]))];

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
        return;
    }

    let items: Vec<ListItem> = unhealthy
        .iter()
        .map(|(module, topic)| {
            let status_style = app.theme.status_style(topic.status());
            let status_symbol = match topic.status() {
                HealthStatus::Critical => "!",
                HealthStatus::Warning => "~",
                HealthStatus::Healthy => " ",
            };

            let pending_info = topic
                .pending_for()
                .map(|d| format!(" (pending: {})", format_duration(d)))
                .unwrap_or_default();

            let unread_info = if let UnhealthyTopic::Read(r) = topic {
                r.unread.filter(|&u| u > 0).map(|u| format!(" (unread: {})", u)).unwrap_or_default()
            } else {
                String::new()
            };

            let line = Line::from(vec![
                Span::styled(
                    format!(" {} ", status_symbol),
                    status_style.add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("[{}] ", topic.status().symbol()), status_style),
                Span::raw(&module.name),
                Span::styled(" -> ", Style::default().fg(app.theme.border)),
                Span::raw(topic.topic()),
                Span::styled(
                    format!(" ({})", topic.kind()),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled(pending_info, status_style),
                Span::styled(unread_info, status_style),
            ]);

            ListItem::new(line)
        })
        .collect();

    let block = Block::default()
        .title(format!(" Bottlenecks ({}) ", unhealthy.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));

    let list =
        List::new(items).block(block).highlight_style(app.theme.selected).highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(
        app.selected_topic_index.min(unhealthy.len().saturating_sub(1)),
    ));

    frame.render_stateful_widget(list, area, &mut state);
}
