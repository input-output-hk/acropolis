use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use crate::data::DataFlowGraph;
use std::collections::HashSet;

/// Render the data flow as an adjacency matrix showing module-to-module communication
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref data) = app.data else {
        return;
    };

    let graph = DataFlowGraph::from_monitor_data(data);

    // Get all module names
    let module_names: Vec<&str> = data.modules.iter().map(|m| m.name.as_str()).collect();

    if module_names.is_empty() {
        let block = Block::default()
            .title(" Data Flow ")
            .borders(Borders::ALL)
            .border_type(app.theme.border_type)
            .border_style(Style::default().fg(app.theme.border));
        let paragraph = Paragraph::new("No modules loaded").block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    // Build adjacency: for each pair (row_module, col_module), determine relationship
    // → = row writes to a topic that col reads
    // ← = row reads from a topic that col writes
    // ↔ = both directions

    // First, build topic relationships
    // writes[module] = set of topics it writes to
    // reads[module] = set of topics it reads from
    let mut writes_to: std::collections::HashMap<&str, HashSet<&str>> =
        std::collections::HashMap::new();
    let mut reads_from: std::collections::HashMap<&str, HashSet<&str>> =
        std::collections::HashMap::new();

    for module in &data.modules {
        let w: HashSet<&str> = module.writes.iter().map(|w| w.topic.as_str()).collect();
        let r: HashSet<&str> = module.reads.iter().map(|r| r.topic.as_str()).collect();
        writes_to.insert(&module.name, w);
        reads_from.insert(&module.name, r);
    }

    // Calculate column width based on longest module name
    let max_name_len = module_names.iter().map(|n| n.len()).max().unwrap_or(10);
    let col_width = max_name_len.min(12) + 1;
    let row_header_width = max_name_len.min(14) + 2;

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    // Header row with column module names (abbreviated)
    let mut header_spans: Vec<Span> = vec![
        Span::raw(format!(
            "{:row_header_width$}",
            "",
            row_header_width = row_header_width
        )),
        Span::styled("│", Style::default().fg(app.theme.border)),
    ];

    for (col_idx, col_name) in module_names.iter().enumerate() {
        let display = truncate(col_name, col_width - 1);
        let is_selected = col_idx == app.selected_module_index;
        let style = if is_selected {
            Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        header_spans.push(Span::styled(
            format!("{:^col_width$}", display, col_width = col_width),
            style,
        ));
    }
    lines.push(Line::from(header_spans));

    // Separator line
    lines.push(Line::from(vec![Span::styled(
        format!(
            "{:─<row_header_width$}┼{:─<rest$}",
            "",
            "",
            row_header_width = row_header_width,
            rest = col_width * module_names.len()
        ),
        Style::default().fg(app.theme.border),
    )]));

    // Data rows
    for (row_idx, row_name) in module_names.iter().enumerate() {
        let is_row_selected = row_idx == app.selected_module_index;
        let row_style = if is_row_selected {
            Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let mut row_spans: Vec<Span> = vec![
            Span::styled(
                format!(
                    "{:>row_header_width$}",
                    truncate(row_name, row_header_width - 1),
                    row_header_width = row_header_width
                ),
                row_style,
            ),
            Span::styled("│", Style::default().fg(app.theme.border)),
        ];

        let row_writes = writes_to.get(row_name).cloned().unwrap_or_default();
        let row_reads = reads_from.get(row_name).cloned().unwrap_or_default();

        for (col_idx, col_name) in module_names.iter().enumerate() {
            if row_idx == col_idx {
                // Self - show dot
                row_spans.push(Span::styled(
                    format!("{:^col_width$}", "·", col_width = col_width),
                    Style::default().add_modifier(Modifier::DIM),
                ));
                continue;
            }

            let col_writes = writes_to.get(col_name).cloned().unwrap_or_default();
            let col_reads = reads_from.get(col_name).cloned().unwrap_or_default();

            // row → col: row writes to a topic that col reads
            let row_to_col = row_writes.iter().any(|topic| col_reads.contains(topic));
            // col → row: col writes to a topic that row reads
            let col_to_row = col_writes.iter().any(|topic| row_reads.contains(topic));

            let (symbol, style) = match (row_to_col, col_to_row) {
                (true, true) => ("↔", Style::default().fg(app.theme.highlight)),
                (true, false) => ("→", Style::default().fg(app.theme.healthy)),
                (false, true) => ("←", Style::default().fg(app.theme.warning)),
                (false, false) => ("", Style::default().add_modifier(Modifier::DIM)),
            };

            row_spans.push(Span::styled(
                format!("{:^col_width$}", symbol, col_width = col_width),
                style,
            ));
        }

        lines.push(Line::from(row_spans));
    }

    // Legend
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" Legend: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("→", Style::default().fg(app.theme.healthy)),
        Span::raw(" sends to  "),
        Span::styled("←", Style::default().fg(app.theme.warning)),
        Span::raw(" receives from  "),
        Span::styled("↔", Style::default().fg(app.theme.highlight)),
        Span::raw(" bidirectional  "),
        Span::styled("·", Style::default().add_modifier(Modifier::DIM)),
        Span::raw(" self"),
    ]));

    // Topic detail for selected module
    lines.push(Line::from(""));
    if let Some(selected) = data.modules.get(app.selected_module_index) {
        lines.push(Line::from(vec![Span::styled(
            format!(" {} connections:", selected.name),
            Style::default().add_modifier(Modifier::BOLD),
        )]));

        // Show what this module writes to and who reads it
        for w in &selected.writes {
            let consumers: Vec<&str> = graph
                .consumers
                .get(&w.topic)
                .map(|v| {
                    v.iter().map(|s| s.as_str()).filter(|s| *s != selected.name.as_str()).collect()
                })
                .unwrap_or_default();

            if !consumers.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(&selected.name, Style::default().fg(app.theme.highlight)),
                    Span::styled(" ──", Style::default().fg(app.theme.border)),
                    Span::styled(
                        truncate(&w.topic, 25),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                    Span::styled("──► ", Style::default().fg(app.theme.border)),
                    Span::raw(consumers.join(", ")),
                ]));
            }
        }

        // Show what this module reads from and who writes it
        for r in &selected.reads {
            let producers: Vec<&str> = graph
                .producers
                .get(&r.topic)
                .map(|v| {
                    v.iter().map(|s| s.as_str()).filter(|s| *s != selected.name.as_str()).collect()
                })
                .unwrap_or_default();

            if !producers.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::raw(producers.join(", ")),
                    Span::styled(" ──", Style::default().fg(app.theme.border)),
                    Span::styled(
                        truncate(&r.topic, 25),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                    Span::styled("──► ", Style::default().fg(app.theme.border)),
                    Span::styled(&selected.name, Style::default().fg(app.theme.highlight)),
                ]));
            }
        }
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        " ↑/↓: select module   Enter: details",
        Style::default().add_modifier(Modifier::DIM),
    )]));

    let title = " Data Flow (Adjacency Matrix) ";

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
