use ratatui::{
    layout::{Constraint, Layout, Rect},
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

    // Calculate responsive column width based on terminal width and module count
    let available_width = area.width as usize;
    let row_header_w = 14usize;
    let matrix_overhead = row_header_w + 4; // borders and padding
    let remaining_for_cols = available_width.saturating_sub(matrix_overhead);
    let col_w = if !module_names.is_empty() {
        (remaining_for_cols / module_names.len()).clamp(4, 10)
    } else {
        6
    };

    // Calculate matrix height: header(1) + border(1) + rows(n) + border(1)
    let matrix_height = (module_names.len() + 3).min(area.height as usize / 2);

    // Split area: matrix on top, details panel below
    let chunks = Layout::vertical([
        Constraint::Length(matrix_height as u16 + 2), // +2 for block borders
        Constraint::Min(6),                           // Details panel fills remaining space
    ])
    .split(area);

    // ===== RENDER MATRIX PANEL =====
    let mut matrix_lines: Vec<Line> = Vec::new();

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
    matrix_lines.push(Line::from(header));

    // Top border of matrix
    let matrix_width = col_w * module_names.len();
    matrix_lines.push(Line::from(vec![Span::styled(
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
        matrix_lines.push(Line::from(row));
    }

    // Bottom border of matrix
    matrix_lines.push(Line::from(vec![Span::styled(
        format!(
            "{:─<row_header_w$}┴{:─<matrix_width$}╯",
            "",
            "",
            row_header_w = row_header_w,
            matrix_width = matrix_width
        ),
        Style::default().fg(app.theme.border),
    )]));

    let matrix_block = Block::default()
        .title(format!(" Data Flow ({} modules) ", module_names.len()))
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.highlight));

    let matrix_paragraph = Paragraph::new(matrix_lines).block(matrix_block);
    frame.render_widget(matrix_paragraph, chunks[0]);

    // ===== RENDER DETAILS PANEL =====
    let details_width = chunks[1].width.saturating_sub(2) as usize; // Account for borders
    let mut detail_lines: Vec<Line> = Vec::new();

    // Legend row
    detail_lines.push(Line::from(vec![
        Span::styled(" Legend: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("→", Style::default().fg(app.theme.healthy)),
        Span::raw(" sends  "),
        Span::styled("←", Style::default().fg(app.theme.warning)),
        Span::raw(" receives  "),
        Span::styled("↔", Style::default().fg(app.theme.highlight)),
        Span::raw(" both  "),
        Span::styled("·", Style::default().add_modifier(Modifier::DIM)),
        Span::raw(" self"),
    ]));
    detail_lines.push(Line::from(""));

    // Selected module connections
    if let Some(selected) = data.modules.get(app.selected_module_index) {
        // Module header with stats
        let total_out: usize = selected
            .writes
            .iter()
            .filter_map(|w| graph.consumers.get(&w.topic).map(|c| c.len().saturating_sub(1)))
            .sum();
        let total_in: usize = selected
            .reads
            .iter()
            .filter_map(|r| graph.producers.get(&r.topic).map(|p| p.len().saturating_sub(1)))
            .sum();

        detail_lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", selected.name),
                Style::default().fg(app.theme.highlight).add_modifier(Modifier::BOLD),
            ),
            Span::styled("│ ", Style::default().fg(app.theme.border)),
            Span::styled(
                format!("{}→ ", total_out),
                Style::default().fg(app.theme.healthy),
            ),
            Span::raw("out  "),
            Span::styled(
                format!("{}← ", total_in),
                Style::default().fg(app.theme.warning),
            ),
            Span::raw("in  "),
            Span::styled("│ ", Style::default().fg(app.theme.border)),
            Span::styled(
                format!("R:{} W:{}", selected.reads.len(), selected.writes.len()),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]));

        detail_lines.push(Line::from(vec![Span::styled(
            format!(" {:─<w$}", "", w = details_width.saturating_sub(2)),
            Style::default().fg(app.theme.border),
        )]));

        // Outgoing connections
        let mut has_connections = false;
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
                let topic_display = truncate(&w.topic, 25);
                let consumers_display =
                    truncate(&consumers.join(", "), details_width.saturating_sub(35));
                detail_lines.push(Line::from(vec![
                    Span::styled(" → ", Style::default().fg(app.theme.healthy)),
                    Span::styled(
                        format!("{:<25}", topic_display),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                    Span::raw(" → "),
                    Span::raw(consumers_display),
                ]));
            }
        }

        // Incoming connections
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
                let topic_display = truncate(&r.topic, 25);
                let producers_display =
                    truncate(&producers.join(", "), details_width.saturating_sub(35));
                detail_lines.push(Line::from(vec![
                    Span::styled(" ← ", Style::default().fg(app.theme.warning)),
                    Span::styled(
                        format!("{:<25}", topic_display),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                    Span::raw(" ← "),
                    Span::raw(producers_display),
                ]));
            }
        }

        if !has_connections {
            detail_lines.push(Line::from(vec![Span::styled(
                "   (no external connections)",
                Style::default().add_modifier(Modifier::DIM),
            )]));
        }
    }

    // Footer with controls
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(vec![Span::styled(
        " ↑/↓ select    Enter details    Tab switch view",
        Style::default().add_modifier(Modifier::DIM),
    )]));

    let details_block = Block::default()
        .title(" Module Connections ")
        .borders(Borders::ALL)
        .border_type(app.theme.border_type)
        .border_style(Style::default().fg(app.theme.border));

    let details_paragraph = Paragraph::new(detail_lines).block(details_block);
    frame.render_widget(details_paragraph, chunks[1]);
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}
