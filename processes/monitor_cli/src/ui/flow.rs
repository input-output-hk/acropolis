use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use crate::data::DataFlowGraph;

/// Render the data flow view as a clean dependency graph
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let Some(module) = data.modules.get(app.selected_module_index) else {
        return;
    };

    let graph = DataFlowGraph::from_monitor_data(data);

    // Find topics this module reads from (inputs) and writes to (outputs)
    let input_topics: Vec<&str> = module.reads.iter().map(|r| r.topic.as_str()).collect();
    let output_topics: Vec<&str> = module.writes.iter().map(|w| w.topic.as_str()).collect();

    // Find upstream producers (modules that write to topics we read)
    let mut upstream_modules: Vec<&str> = Vec::new();
    for topic in &input_topics {
        if let Some(producers) = graph.producers.get(*topic) {
            for p in producers {
                if p != &module.name && !upstream_modules.contains(&p.as_str()) {
                    upstream_modules.push(p.as_str());
                }
            }
        }
    }

    // Find downstream consumers (modules that read from topics we write)
    let mut downstream_modules: Vec<&str> = Vec::new();
    for topic in &output_topics {
        if let Some(consumers) = graph.consumers.get(*topic) {
            for c in consumers {
                if c != &module.name && !downstream_modules.contains(&c.as_str()) {
                    downstream_modules.push(c.as_str());
                }
            }
        }
    }

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    // === UPSTREAM MODULES ===
    if !upstream_modules.is_empty() {
        lines.extend(render_module_row(&upstream_modules, app, false));
        lines.push(render_arrows_down(upstream_modules.len()));
    }

    // === INPUT TOPICS ===
    if !input_topics.is_empty() {
        lines.extend(render_topic_boxes(&input_topics, app));
        lines.push(render_arrow_down_single());
    }

    // === MAIN MODULE (highlighted) ===
    lines.extend(render_main_module(&module.name, app));

    // === OUTPUT TOPICS ===
    if !output_topics.is_empty() {
        lines.push(render_arrow_down_single());
        lines.extend(render_topic_boxes(&output_topics, app));
    }

    // === DOWNSTREAM MODULES ===
    if !downstream_modules.is_empty() {
        lines.push(render_arrows_down(downstream_modules.len()));
        lines.extend(render_module_row(&downstream_modules, app, false));
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        " ↑/↓: change module   Enter: details",
        Style::default().add_modifier(Modifier::DIM),
    )]));

    let title = format!(" Data Flow: {} ", module.name);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.highlight));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Render a row of module boxes
fn render_module_row(modules: &[&str], app: &App, _highlighted: bool) -> Vec<Line<'static>> {
    if modules.is_empty() {
        return vec![];
    }

    let box_width = 16;
    let total_width = modules.len() * (box_width + 2);
    let left_pad = 40usize.saturating_sub(total_width / 2);
    let padding = " ".repeat(left_pad);

    let mut top_line: Vec<Span> = vec![Span::raw(padding.clone())];
    let mut mid_line: Vec<Span> = vec![Span::raw(padding.clone())];
    let mut bot_line: Vec<Span> = vec![Span::raw(padding.clone())];

    for (i, name) in modules.iter().enumerate() {
        if i > 0 {
            top_line.push(Span::raw("  "));
            mid_line.push(Span::raw("  "));
            bot_line.push(Span::raw("  "));
        }

        let display_name = truncate(name, box_width - 4);
        let name_pad = (box_width - 2).saturating_sub(display_name.len()) / 2;
        let name_pad_right = (box_width - 2).saturating_sub(display_name.len()) - name_pad;

        let style = Style::default().fg(app.theme.border);

        top_line.push(Span::styled(
            format!("┌{}┐", "─".repeat(box_width - 2)),
            style,
        ));
        mid_line.push(Span::styled("│".to_string(), style));
        mid_line.push(Span::raw(format!("{:>w$}", "", w = name_pad)));
        mid_line.push(Span::raw(display_name));
        mid_line.push(Span::raw(format!("{:<w$}", "", w = name_pad_right)));
        mid_line.push(Span::styled("│".to_string(), style));
        bot_line.push(Span::styled(
            format!("└{}┘", "─".repeat(box_width - 2)),
            style,
        ));
    }

    vec![
        Line::from(top_line),
        Line::from(mid_line),
        Line::from(bot_line),
    ]
}

/// Render topic boxes (can show multiple topics)
fn render_topic_boxes(topics: &[&str], app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for topic in topics {
        let box_width = topic.len().max(20) + 4;
        let left_pad = 40usize.saturating_sub(box_width / 2);
        let padding = " ".repeat(left_pad);

        let style = Style::default().fg(app.theme.border);

        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(format!("┌{}┐", "─".repeat(box_width - 2)), style),
        ]));

        let topic_display = truncate(topic, box_width - 4);
        let topic_pad = (box_width - 2).saturating_sub(topic_display.len()) / 2;
        let topic_pad_right = (box_width - 2).saturating_sub(topic_display.len()) - topic_pad;

        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled("│".to_string(), style),
            Span::raw(format!("{:>w$}", "", w = topic_pad)),
            Span::styled(topic_display, Style::default().add_modifier(Modifier::DIM)),
            Span::raw(format!("{:<w$}", "", w = topic_pad_right)),
            Span::styled("│".to_string(), style),
        ]));

        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(format!("└{}┘", "─".repeat(box_width - 2)), style),
        ]));
    }

    lines
}

/// Render the main highlighted module
fn render_main_module(name: &str, app: &App) -> Vec<Line<'static>> {
    let box_width = name.len().max(14) + 6;
    let left_pad = 40usize.saturating_sub(box_width / 2);
    let padding = " ".repeat(left_pad);

    let style = Style::default().fg(app.theme.highlight);
    let name_pad = (box_width - 2).saturating_sub(name.len()) / 2;
    let name_pad_right = (box_width - 2).saturating_sub(name.len()) - name_pad;

    vec![
        Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(format!("╔{}╗", "═".repeat(box_width - 2)), style),
        ]),
        Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled("║".to_string(), style),
            Span::raw(format!("{:>w$}", "", w = name_pad)),
            Span::styled(name.to_string(), style.add_modifier(Modifier::BOLD)),
            Span::raw(format!("{:<w$}", "", w = name_pad_right)),
            Span::styled("║".to_string(), style),
        ]),
        Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(format!("╚{}╝", "═".repeat(box_width - 2)), style),
        ]),
    ]
}

/// Render multiple arrows pointing down
fn render_arrows_down(count: usize) -> Line<'static> {
    if count == 0 {
        return Line::from("");
    }

    let box_width = 16;
    let total_width = count * (box_width + 2);
    let left_pad = 40usize.saturating_sub(total_width / 2);
    let padding = " ".repeat(left_pad);

    let mut spans: Vec<Span> = vec![Span::raw(padding)];

    for i in 0..count {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        let arrow_pad = box_width / 2;
        spans.push(Span::styled(
            format!("{:>w$}│{:<w2$}", "", "", w = arrow_pad - 1, w2 = arrow_pad),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    Line::from(spans)
}

/// Render a single centered arrow pointing down
fn render_arrow_down_single() -> Line<'static> {
    Line::from(vec![Span::styled(
        "                                        │",
        Style::default().add_modifier(Modifier::DIM),
    )])
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}
