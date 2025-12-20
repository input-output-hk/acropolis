use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use crate::data::{DataFlowGraph, HealthStatus};

/// Render the module-centric data flow view with ASCII boxes
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let Some(module) = data.modules.get(app.selected_module_index) else {
        return;
    };

    let graph = DataFlowGraph::from_monitor_data(data);

    // Find topics this module reads from (inputs) and writes to (outputs)
    let mut inputs: Vec<(&str, Vec<&str>)> = Vec::new(); // (topic, producers)
    let mut outputs: Vec<(&str, Vec<&str>)> = Vec::new(); // (topic, consumers)

    for read in &module.reads {
        let producers: Vec<&str> = graph
            .producers
            .get(&read.topic)
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        inputs.push((&read.topic, producers));
    }

    for write in &module.writes {
        let consumers: Vec<&str> = graph
            .consumers
            .get(&write.topic)
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        outputs.push((&write.topic, consumers));
    }

    // Build the ASCII art display
    let mut lines: Vec<Line> = Vec::new();

    // Module header box
    let module_box_width = module.name.len() + 4;
    let health_style = app.theme.status_style(module.health);

    lines.push(Line::from(""));

    // Center the module box
    let center_padding = (area.width as usize).saturating_sub(module_box_width) / 2;
    let pad = " ".repeat(center_padding);

    lines.push(Line::from(vec![
        Span::raw(pad.clone()),
        Span::styled(
            format!("╭{}╮", "─".repeat(module_box_width - 2)),
            Style::default().fg(app.theme.highlight),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::raw(pad.clone()),
        Span::styled("│ ", Style::default().fg(app.theme.highlight)),
        Span::styled(
            module.name.clone(),
            Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │", Style::default().fg(app.theme.highlight)),
    ]));

    lines.push(Line::from(vec![
        Span::raw(pad.clone()),
        Span::styled("│", Style::default().fg(app.theme.highlight)),
        Span::styled(
            format!(
                " R:{:<5} W:{:<5}",
                format_count(module.total_read),
                format_count(module.total_written)
            ),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::styled("│", Style::default().fg(app.theme.highlight)),
    ]));

    lines.push(Line::from(vec![
        Span::raw(pad.clone()),
        Span::styled("│", Style::default().fg(app.theme.highlight)),
        Span::raw("    "),
        Span::styled(module.health.symbol(), health_style),
        Span::raw("      "),
        Span::styled("│", Style::default().fg(app.theme.highlight)),
    ]));

    lines.push(Line::from(vec![
        Span::raw(pad.clone()),
        Span::styled(
            format!("╰{}╯", "─".repeat(module_box_width - 2)),
            Style::default().fg(app.theme.highlight),
        ),
    ]));

    lines.push(Line::from(""));

    // Show inputs and outputs sections
    let section_width = (area.width as usize).saturating_sub(4) / 2;

    // Divider
    lines.push(Line::from(vec![Span::styled(
        format!("  {:─<width$}┬{:─<width$}", "", "", width = section_width),
        Style::default().fg(app.theme.border),
    )]));

    // Section headers
    lines.push(Line::from(vec![
        Span::styled(
            format!(
                "  {:^width$}",
                format!("INPUTS ({})", inputs.len()),
                width = section_width
            ),
            app.theme.header,
        ),
        Span::styled("│", Style::default().fg(app.theme.border)),
        Span::styled(
            format!(
                "{:^width$}",
                format!("OUTPUTS ({})", outputs.len()),
                width = section_width
            ),
            app.theme.header,
        ),
    ]));

    lines.push(Line::from(vec![Span::styled(
        format!("  {:─<width$}┼{:─<width$}", "", "", width = section_width),
        Style::default().fg(app.theme.border),
    )]));

    // Render inputs and outputs side by side
    let max_rows = inputs.len().max(outputs.len()).max(1);

    for i in 0..max_rows {
        let mut spans: Vec<Span> = Vec::new();

        // Left side (inputs)
        if let Some((topic, producers)) = inputs.get(i) {
            let producer_str = if producers.is_empty() {
                "(external)".to_string()
            } else {
                producers.join(", ")
            };

            // Find the read data for health info
            let read_data = module.reads.iter().find(|r| &r.topic == *topic);
            let status = read_data.map(|r| r.status).unwrap_or(HealthStatus::Healthy);
            let pending = read_data
                .and_then(|r| r.pending_for)
                .map(|d| crate::data::duration::format_duration(d))
                .unwrap_or_else(|| "-".to_string());
            let topic_truncated = truncate(topic, 25);
            let producer_truncated = truncate(&producer_str, 15);

            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("{:<15}", producer_truncated),
                Style::default().add_modifier(Modifier::DIM),
            ));
            spans.push(Span::styled(" → ", Style::default().fg(app.theme.border)));
            spans.push(Span::styled(
                format!("{:<25}", topic_truncated),
                Style::default(),
            ));
            spans.push(Span::styled(
                format!(" {:>6} ", pending),
                app.theme.status_style(status),
            ));
            spans.push(Span::styled(
                status.symbol(),
                app.theme.status_style(status),
            ));
        } else {
            spans.push(Span::raw(format!("  {:width$}", "", width = section_width)));
        }

        spans.push(Span::styled("│", Style::default().fg(app.theme.border)));

        // Right side (outputs)
        if let Some((topic, consumers)) = outputs.get(i) {
            let consumer_str = if consumers.is_empty() {
                "(no consumers)".to_string()
            } else {
                consumers.join(", ")
            };

            // Find the write data for health info
            let write_data = module.writes.iter().find(|w| &w.topic == *topic);
            let status = write_data.map(|w| w.status).unwrap_or(HealthStatus::Healthy);
            let pending = write_data
                .and_then(|w| w.pending_for)
                .map(|d| crate::data::duration::format_duration(d))
                .unwrap_or_else(|| "-".to_string());

            let topic_truncated = truncate(topic, 25);
            let consumer_truncated = truncate(&consumer_str, 15);

            spans.push(Span::styled(
                status.symbol(),
                app.theme.status_style(status),
            ));
            spans.push(Span::styled(
                format!(" {:>6}", pending),
                app.theme.status_style(status),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{:<25}", topic_truncated),
                Style::default(),
            ));
            spans.push(Span::styled(" → ", Style::default().fg(app.theme.border)));
            spans.push(Span::styled(
                format!("{:<15}", consumer_truncated),
                Style::default().add_modifier(Modifier::DIM),
            ));
        }

        lines.push(Line::from(spans));
    }

    // Footer with navigation hint
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        " ↑/↓: select module  Enter: details  /: filter ",
        Style::default().add_modifier(Modifier::DIM),
    )]));

    // Build the block
    let selected_name = &module.name;
    let title = format!(
        " Data Flow: {} ({} inputs, {} outputs) ",
        selected_name,
        inputs.len(),
        outputs.len()
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.highlight));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
