use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::app::App;
use crate::data::DataFlowGraph;

/// Render the data flow view with module highlighting
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let graph = DataFlowGraph::from_monitor_data(data);
    let lines = graph.to_lines();

    // Get the currently selected module name (from summary view selection)
    let selected_module = data.modules.get(app.selected_module_index).map(|m| &m.name);

    // Filter lines based on filter text or show all
    let filtered_lines: Vec<_> = if app.filter_text.is_empty() {
        lines.iter().enumerate().collect()
    } else {
        lines
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.topic.to_lowercase().contains(&app.filter_text.to_lowercase())
                    || l.producers
                        .iter()
                        .any(|p| p.to_lowercase().contains(&app.filter_text.to_lowercase()))
                    || l.consumers
                        .iter()
                        .any(|c| c.to_lowercase().contains(&app.filter_text.to_lowercase()))
            })
            .collect()
    };

    // Calculate column widths
    let max_producer_width = filtered_lines
        .iter()
        .map(|(_, l)| {
            if l.producers.is_empty() {
                6 // "(none)"
            } else {
                l.producers.join(", ").len()
            }
        })
        .max()
        .unwrap_or(10)
        .min(30);

    let max_topic_width =
        filtered_lines.iter().map(|(_, l)| l.topic.len()).max().unwrap_or(20).min(40);

    let items: Vec<ListItem> = filtered_lines
        .iter()
        .map(|(_, line)| {
            // Check if this topic is connected to the selected module
            let is_connected = selected_module.map_or(false, |m| {
                line.producers.contains(m) || line.consumers.contains(m)
            });

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

            // Style based on connection to selected module
            let (producer_style, arrow_style, topic_style, consumer_style) = if is_connected {
                (
                    Style::default().fg(app.theme.highlight),
                    Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD),
                    Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD),
                    Style::default().fg(app.theme.highlight),
                )
            } else {
                (
                    Style::default().add_modifier(Modifier::DIM),
                    Style::default().fg(app.theme.border),
                    Style::default(),
                    Style::default().add_modifier(Modifier::DIM),
                )
            };

            // Highlight the selected module name in producers/consumers
            let producer_spans = if let Some(module) = selected_module {
                highlight_module(&producers, module, producer_style, app.theme.healthy)
            } else {
                vec![Span::styled(
                    format!("{:>width$}", producers, width = max_producer_width),
                    producer_style,
                )]
            };

            let consumer_spans = if let Some(module) = selected_module {
                highlight_module(&consumers, module, consumer_style, app.theme.healthy)
            } else {
                vec![Span::styled(consumers, consumer_style)]
            };

            let mut spans = producer_spans;
            spans.push(Span::styled(" → ", arrow_style));
            spans.push(Span::styled(
                format!("{:<width$}", topic_display, width = max_topic_width),
                topic_style,
            ));
            spans.push(Span::styled(" → ", arrow_style));
            spans.extend(consumer_spans);

            ListItem::new(Line::from(spans))
        })
        .collect();

    // Build title
    let filter_info = if !app.filter_text.is_empty() {
        format!(" /{}/", app.filter_text)
    } else {
        String::new()
    };

    let module_info = selected_module.map(|m| format!(" [{}]", m)).unwrap_or_default();

    let title = format!(
        " Data Flow ({}/{} topics){}{} ",
        filtered_lines.len(),
        lines.len(),
        filter_info,
        module_info
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.border));

    let list =
        List::new(items).block(block).highlight_style(app.theme.selected).highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(
        app.selected_topic_index.min(filtered_lines.len().saturating_sub(1)),
    ));

    frame.render_stateful_widget(list, area, &mut state);
}

/// Highlight occurrences of a module name in text
fn highlight_module(
    text: &str,
    module: &str,
    base_style: Style,
    highlight_color: ratatui::style::Color,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let text_lower = text.to_lowercase();
    let module_lower = module.to_lowercase();

    let mut last_end = 0;
    for (start, _) in text_lower.match_indices(&module_lower) {
        if start > last_end {
            spans.push(Span::styled(text[last_end..start].to_string(), base_style));
        }
        spans.push(Span::styled(
            text[start..start + module.len()].to_string(),
            Style::default().fg(highlight_color).add_modifier(Modifier::BOLD),
        ));
        last_end = start + module.len();
    }
    if last_end < text.len() {
        spans.push(Span::styled(text[last_end..].to_string(), base_style));
    }

    if spans.is_empty() {
        spans.push(Span::styled(text.to_string(), base_style));
    }

    spans
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}
