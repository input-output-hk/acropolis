use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use crate::data::{DataFlowGraph, HealthStatus};

/// Render the data flow view as a dependency graph centered on the selected module
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let Some(module) = data.modules.get(app.selected_module_index) else {
        return;
    };

    let graph = DataFlowGraph::from_monitor_data(data);

    // Find topics this module reads from (inputs) and writes to (outputs)
    let mut inputs: Vec<InputInfo> = Vec::new();
    let mut outputs: Vec<OutputInfo> = Vec::new();

    for read in &module.reads {
        let producers: Vec<String> = graph.producers.get(&read.topic).cloned().unwrap_or_default();
        inputs.push(InputInfo {
            topic: read.topic.clone(),
            producers,
            status: read.status,
            pending: read.pending_for,
            unread: read.unread,
        });
    }

    for write in &module.writes {
        let consumers: Vec<String> = graph.consumers.get(&write.topic).cloned().unwrap_or_default();
        outputs.push(OutputInfo {
            topic: write.topic.clone(),
            consumers,
            status: write.status,
            pending: write.pending_for,
        });
    }

    let mut lines: Vec<Line> = Vec::new();
    let health_style = app.theme.status_style(module.health);

    // === UPSTREAM SECTION (modules that feed into this one) ===
    lines.push(Line::from(""));

    if inputs.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  (no inputs)",
            Style::default().add_modifier(Modifier::DIM),
        )]));
    } else {
        // Show upstream producers
        let mut all_producers: Vec<&str> =
            inputs.iter().flat_map(|i| i.producers.iter().map(|s| s.as_str())).collect();
        all_producers.sort();
        all_producers.dedup();

        if !all_producers.is_empty() {
            let producers_line = all_producers
                .iter()
                .map(|p| {
                    // Check if this producer has health issues
                    let producer_module = data.modules.iter().find(|m| &m.name == *p);
                    let style = producer_module
                        .map(|m| app.theme.status_style(m.health))
                        .unwrap_or_default();
                    (p.to_string(), style)
                })
                .collect::<Vec<_>>();

            let mut spans: Vec<Span> = vec![Span::raw("  ")];
            for (i, (name, style)) in producers_line.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled("  ", Style::default()));
                }
                spans.push(Span::styled(
                    format!("┌{}┐", "─".repeat(name.len() + 2)),
                    style.add_modifier(Modifier::DIM),
                ));
            }
            lines.push(Line::from(spans));

            let mut spans: Vec<Span> = vec![Span::raw("  ")];
            for (i, (name, style)) in producers_line.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled("  ", Style::default()));
                }
                spans.push(Span::styled(format!("│ {} │", name), *style));
            }
            lines.push(Line::from(spans));

            let mut spans: Vec<Span> = vec![Span::raw("  ")];
            for (i, (name, style)) in producers_line.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled("  ", Style::default()));
                }
                spans.push(Span::styled(
                    format!("└{}┘", "─".repeat(name.len() + 2)),
                    style.add_modifier(Modifier::DIM),
                ));
            }
            lines.push(Line::from(spans));

            // Arrows pointing down
            let mut spans: Vec<Span> = vec![Span::raw("  ")];
            for (i, (name, _)) in producers_line.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled("  ", Style::default()));
                }
                let padding = (name.len() + 4) / 2;
                spans.push(Span::styled(
                    format!(
                        "{:>width$}│{:<width2$}",
                        "",
                        "",
                        width = padding,
                        width2 = padding
                    ),
                    Style::default().fg(app.theme.border),
                ));
            }
            lines.push(Line::from(spans));

            let mut spans: Vec<Span> = vec![Span::raw("  ")];
            for (i, (name, _)) in producers_line.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled("  ", Style::default()));
                }
                let padding = (name.len() + 4) / 2;
                spans.push(Span::styled(
                    format!(
                        "{:>width$}▼{:<width2$}",
                        "",
                        "",
                        width = padding,
                        width2 = padding
                    ),
                    Style::default().fg(app.theme.border),
                ));
            }
            lines.push(Line::from(spans));
        }

        // Show input topics
        for input in &inputs {
            let status_style = app.theme.status_style(input.status);
            let pending = input
                .pending
                .map(|d| crate::data::duration::format_duration(d))
                .unwrap_or_else(|| "-".to_string());
            let unread = input.unread.map(|u| u.to_string()).unwrap_or_else(|| "-".to_string());

            lines.push(Line::from(vec![
                Span::styled("  ╭─ ", Style::default().fg(app.theme.border)),
                Span::styled(truncate(&input.topic, 35), Style::default()),
                Span::styled(" ─╮", Style::default().fg(app.theme.border)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  │  ", Style::default().fg(app.theme.border)),
                Span::styled(
                    format!("pending: {:>8}  unread: {:>6}  ", pending, unread),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled(input.status.symbol(), status_style),
                Span::styled(" │", Style::default().fg(app.theme.border)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  ╰", Style::default().fg(app.theme.border)),
                Span::styled(
                    "───────────────────────────────────────────",
                    Style::default().fg(app.theme.border),
                ),
                Span::styled("╯", Style::default().fg(app.theme.border)),
            ]));
        }
    }

    // Arrow into main module
    lines.push(Line::from(vec![Span::styled(
        "                    │",
        Style::default().fg(app.theme.border),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "                    ▼",
        Style::default().fg(app.theme.border),
    )]));

    // === MAIN MODULE (center) ===
    let module_width = module.name.len().max(20) + 4;
    let pad_left = 20usize.saturating_sub(module_width / 2);
    let padding = " ".repeat(pad_left);

    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
        Span::styled(
            format!("╔{}╗", "═".repeat(module_width)),
            Style::default().fg(app.theme.highlight),
        ),
    ]));

    let name_pad = module_width.saturating_sub(module.name.len()) / 2;
    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
        Span::styled("║", Style::default().fg(app.theme.highlight)),
        Span::raw(format!("{:>width$}", "", width = name_pad)),
        Span::styled(
            module.name.clone(),
            Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "{:<width$}",
            "",
            width = module_width - name_pad - module.name.len()
        )),
        Span::styled("║", Style::default().fg(app.theme.highlight)),
    ]));

    let stats = format!(
        "R:{} W:{}",
        format_count(module.total_read),
        format_count(module.total_written)
    );
    let stats_pad = module_width.saturating_sub(stats.len()) / 2;
    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
        Span::styled("║", Style::default().fg(app.theme.highlight)),
        Span::raw(format!("{:>width$}", "", width = stats_pad)),
        Span::styled(stats.clone(), Style::default().add_modifier(Modifier::DIM)),
        Span::raw(format!(
            "{:<width$}",
            "",
            width = module_width - stats_pad - stats.len()
        )),
        Span::styled("║", Style::default().fg(app.theme.highlight)),
    ]));

    let health_text = format!(
        "{} {}",
        module.health.symbol(),
        match module.health {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Warning => "warning",
            HealthStatus::Critical => "critical",
        }
    );
    let health_pad = module_width.saturating_sub(health_text.len()) / 2;
    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
        Span::styled("║", Style::default().fg(app.theme.highlight)),
        Span::raw(format!("{:>width$}", "", width = health_pad)),
        Span::styled(health_text.clone(), health_style),
        Span::raw(format!(
            "{:<width$}",
            "",
            width = module_width - health_pad - health_text.len()
        )),
        Span::styled("║", Style::default().fg(app.theme.highlight)),
    ]));

    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
        Span::styled(
            format!("╚{}╝", "═".repeat(module_width)),
            Style::default().fg(app.theme.highlight),
        ),
    ]));

    // Arrow out of main module
    lines.push(Line::from(vec![Span::styled(
        "                    │",
        Style::default().fg(app.theme.border),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "                    ▼",
        Style::default().fg(app.theme.border),
    )]));

    // === DOWNSTREAM SECTION (topics this module writes to) ===
    if outputs.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  (no outputs)",
            Style::default().add_modifier(Modifier::DIM),
        )]));
    } else {
        // Show output topics
        for output in &outputs {
            let status_style = app.theme.status_style(output.status);
            let pending = output
                .pending
                .map(|d| crate::data::duration::format_duration(d))
                .unwrap_or_else(|| "-".to_string());

            lines.push(Line::from(vec![
                Span::styled("  ╭─ ", Style::default().fg(app.theme.border)),
                Span::styled(truncate(&output.topic, 35), Style::default()),
                Span::styled(" ─╮", Style::default().fg(app.theme.border)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  │  ", Style::default().fg(app.theme.border)),
                Span::styled(
                    format!("pending: {:>8}  ", pending),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled(output.status.symbol(), status_style),
                Span::styled(
                    format!(
                        "  → {}",
                        if output.consumers.is_empty() {
                            "(no consumers)".to_string()
                        } else {
                            output.consumers.join(", ")
                        }
                    ),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled(" │", Style::default().fg(app.theme.border)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  ╰", Style::default().fg(app.theme.border)),
                Span::styled(
                    "───────────────────────────────────────────",
                    Style::default().fg(app.theme.border),
                ),
                Span::styled("╯", Style::default().fg(app.theme.border)),
            ]));
        }
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        " ↑/↓: change module   Enter: details   /:filter",
        Style::default().add_modifier(Modifier::DIM),
    )]));

    // Build the block
    let title = format!(" Data Flow: {} ", module.name);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.highlight));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

struct InputInfo {
    topic: String,
    producers: Vec<String>,
    status: HealthStatus,
    pending: Option<std::time::Duration>,
    unread: Option<u64>,
}

struct OutputInfo {
    topic: String,
    consumers: Vec<String>,
    status: HealthStatus,
    pending: Option<std::time::Duration>,
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
