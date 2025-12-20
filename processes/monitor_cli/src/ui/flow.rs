use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::app::App;
use crate::data::DataFlowGraph;

/// Render the data flow view
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let graph = DataFlowGraph::from_monitor_data(data);
    let lines = graph.to_lines();

    // Calculate column widths
    let max_producer_width = lines
        .iter()
        .map(|l| {
            if l.producers.is_empty() {
                6 // "(none)"
            } else {
                l.producers.join(", ").len()
            }
        })
        .max()
        .unwrap_or(10)
        .min(30); // Cap at 30 chars

    let max_topic_width = lines.iter().map(|l| l.topic.len()).max().unwrap_or(20).min(40); // Cap at 40 chars

    let items: Vec<ListItem> = lines
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let producers = if line.producers.is_empty() {
                "(none)".to_string()
            } else {
                truncate(&line.producers.join(", "), max_producer_width)
            };

            let consumers = if line.consumers.is_empty() {
                "(none)".to_string()
            } else {
                line.consumers.join(", ")
            };

            let topic_display = truncate(&line.topic, max_topic_width);

            // Highlight if selected
            let is_selected = idx == app.selected_topic_index;
            let arrow_style = if is_selected {
                Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.border)
            };

            let spans = vec![
                Span::styled(
                    format!("{:>width$}", producers, width = max_producer_width),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled(" -> ", arrow_style),
                Span::styled(
                    format!("{:<width$}", topic_display, width = max_topic_width),
                    Style::default().fg(app.theme.highlight),
                ),
                Span::styled(" -> ", arrow_style),
                Span::raw(consumers),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let block = Block::default()
        .title(format!(" Data Flow ({} topics) ", lines.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));

    let list =
        List::new(items).block(block).highlight_style(app.theme.selected).highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(
        app.selected_topic_index.min(lines.len().saturating_sub(1)),
    ));

    frame.render_stateful_widget(list, area, &mut state);
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
