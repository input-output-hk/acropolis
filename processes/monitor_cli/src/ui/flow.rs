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

    // Build topic relationships
    let mut writes_to: std::collections::HashMap<&str, HashSet<&str>> =
        std::collections::HashMap::new();
    let mut reads_from: std::collections::HashMap<&str, HashSet<&str>> =
        std::collections::HashMap::new();

    for module in &data.modules {
        writes_to.insert(
            &module.name,
            module.writes.iter().map(|w| w.topic.as_str()).collect(),
        );
        reads_from.insert(
            &module.name,
            module.reads.iter().map(|r| r.topic.as_str()).collect(),
        );
    }

    // Layout constants - reduced for tighter spacing
    let col_w = 6usize;
    let row_header_w = 12usize;

    let mut lines: Vec<Line> = Vec::new();

    // Column headers
    let mut header: Vec<Span> = vec![
        Span::raw(format!("{:row_header_w$}", "", row_header_w = row_header_w)),
        Span::styled("│", Style::default().fg(app.theme.border)),
    ];
    for (i, name) in module_names.iter().enumerate() {
        let display = truncate(name, col_w - 1);
        let style = if i == app.selected_module_index {
            Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        header.push(Span::styled(
            format!("{:^col_w$}", display, col_w = col_w),
            style,
        ));
    }
    header.push(Span::styled("│", Style::default().fg(app.theme.border)));
    lines.push(Line::from(header));

    // Top border of matrix
    let matrix_width = col_w * module_names.len();
    lines.push(Line::from(vec![Span::styled(
        format!(
            "{:─<row_header_w$}┼{:─<matrix_width$}┤",
            "",
            "",
            row_header_w = row_header_w,
            matrix_width = matrix_width
        ),
        Style::default().fg(app.theme.border),
    )]));

    // Matrix rows
    for (row_idx, row_name) in module_names.iter().enumerate() {
        let is_selected = row_idx == app.selected_module_index;
        let row_style = if is_selected {
            Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let mut row: Vec<Span> = vec![
            Span::styled(
                format!(
                    "{:>row_header_w$}",
                    truncate(row_name, row_header_w - 1),
                    row_header_w = row_header_w
                ),
                row_style,
            ),
            Span::styled("│", Style::default().fg(app.theme.border)),
        ];

        let row_writes = writes_to.get(row_name).cloned().unwrap_or_default();
        let row_reads = reads_from.get(row_name).cloned().unwrap_or_default();

        for (col_idx, col_name) in module_names.iter().enumerate() {
            if row_idx == col_idx {
                row.push(Span::styled(
                    format!("{:^col_w$}", "·", col_w = col_w),
                    Style::default().add_modifier(Modifier::DIM),
                ));
                continue;
            }

            let col_writes = writes_to.get(col_name).cloned().unwrap_or_default();
            let col_reads = reads_from.get(col_name).cloned().unwrap_or_default();

            let row_to_col = row_writes.iter().any(|t| col_reads.contains(t));
            let col_to_row = col_writes.iter().any(|t| row_reads.contains(t));

            let (symbol, style) = match (row_to_col, col_to_row) {
                (true, true) => ("↔", Style::default().fg(app.theme.highlight)),
                (true, false) => ("→", Style::default().fg(app.theme.healthy)),
                (false, true) => ("←", Style::default().fg(app.theme.warning)),
                (false, false) => ("", Style::default()),
            };

            row.push(Span::styled(
                format!("{:^col_w$}", symbol, col_w = col_w),
                style,
            ));
        }

        row.push(Span::styled("│", Style::default().fg(app.theme.border)));
        lines.push(Line::from(row));
    }

    // Bottom border of matrix
    lines.push(Line::from(vec![Span::styled(
        format!(
            "{:─<row_header_w$}┴{:─<matrix_width$}╯",
            "",
            "",
            row_header_w = row_header_w,
            matrix_width = matrix_width
        ),
        Style::default().fg(app.theme.border),
    )]));

    // Legend with module count
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" Legend: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("→", Style::default().fg(app.theme.healthy)),
        Span::raw(" sends  "),
        Span::styled("←", Style::default().fg(app.theme.warning)),
        Span::raw(" receives  "),
        Span::styled("↔", Style::default().fg(app.theme.highlight)),
        Span::raw(" both  "),
        Span::styled("·", Style::default().add_modifier(Modifier::DIM)),
        Span::raw(" self  │  "),
        Span::styled(
            format!("{} modules", module_names.len()),
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]));

    // Selected module connections
    lines.push(Line::from(""));
    if let Some(selected) = data.modules.get(app.selected_module_index) {
        // Calculate box width based on available space
        let box_width = 55usize;

        lines.push(Line::from(vec![
            Span::styled(
                format!(" ╭─ {} ", selected.name),
                Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{:─<w$}╮",
                    "",
                    w = box_width.saturating_sub(selected.name.len() + 5)
                ),
                Style::default().fg(app.theme.border),
            ),
        ]));

        let mut has_connections = false;

        // Outgoing
        for w in &selected.writes {
            let consumers: Vec<&str> = graph
                .consumers
                .get(&w.topic)
                .map(|v| {
                    v.iter().filter(|s| s.as_str() != selected.name).map(|s| s.as_str()).collect()
                })
                .unwrap_or_default();

            if !consumers.is_empty() {
                has_connections = true;
                lines.push(Line::from(vec![
                    Span::styled(" │  ", Style::default().fg(app.theme.border)),
                    Span::styled("→ ", Style::default().fg(app.theme.healthy)),
                    Span::styled(
                        truncate(&w.topic, 30),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                    Span::raw(" → "),
                    Span::raw(consumers.join(", ")),
                ]));
            }
        }

        // Incoming
        for r in &selected.reads {
            let producers: Vec<&str> = graph
                .producers
                .get(&r.topic)
                .map(|v| {
                    v.iter().filter(|s| s.as_str() != selected.name).map(|s| s.as_str()).collect()
                })
                .unwrap_or_default();

            if !producers.is_empty() {
                has_connections = true;
                lines.push(Line::from(vec![
                    Span::styled(" │  ", Style::default().fg(app.theme.border)),
                    Span::styled("← ", Style::default().fg(app.theme.warning)),
                    Span::raw(producers.join(", ")),
                    Span::raw(" → "),
                    Span::styled(
                        truncate(&r.topic, 30),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                ]));
            }
        }

        if !has_connections {
            lines.push(Line::from(vec![
                Span::styled(" │  ", Style::default().fg(app.theme.border)),
                Span::styled(
                    "(no external connections)",
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ]));
        }

        lines.push(Line::from(vec![
            Span::styled(" ╰", Style::default().fg(app.theme.border)),
            Span::styled(
                format!("{:─<60}", ""),
                Style::default().fg(app.theme.border),
            ),
        ]));
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        " ↑/↓ select module    Enter details    Tab switch view",
        Style::default().add_modifier(Modifier::DIM),
    )]));

    let block = Block::default()
        .title(" Data Flow ")
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
