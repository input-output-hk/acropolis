use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    Terminal,
};

mod app;
mod data;
mod events;
mod ui;

use app::{App, View};

#[derive(Parser, Debug)]
#[command(name = "monitor-cli")]
#[command(about = "TUI monitoring tool for Acropolis processes")]
struct Args {
    /// Path to monitor.json file
    #[arg(short, long, default_value = "monitor.json")]
    file: PathBuf,

    /// Refresh interval in seconds
    #[arg(short, long, default_value = "1")]
    refresh: u64,

    /// Pending duration warning threshold (e.g., "1s", "500ms")
    #[arg(long, default_value = "1s")]
    pending_warn: String,

    /// Pending duration critical threshold (e.g., "10s", "5s")
    #[arg(long, default_value = "10s")]
    pending_crit: String,

    /// Unread count warning threshold
    #[arg(long, default_value = "1000")]
    unread_warn: u64,

    /// Unread count critical threshold
    #[arg(long, default_value = "5000")]
    unread_crit: u64,

    /// Export current state to JSON file and exit
    #[arg(short, long)]
    export: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Parse threshold durations
    let pending_warn = data::duration::parse_duration(&args.pending_warn)
        .unwrap_or(std::time::Duration::from_secs(1));
    let pending_crit = data::duration::parse_duration(&args.pending_crit)
        .unwrap_or(std::time::Duration::from_secs(10));

    let thresholds = data::Thresholds {
        pending_warning: pending_warn,
        pending_critical: pending_crit,
        unread_warning: args.unread_warn,
        unread_critical: args.unread_crit,
    };

    // Handle export mode (non-interactive)
    if let Some(export_path) = args.export {
        return export_to_file(&args.file, &export_path, &thresholds);
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Setup panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic);
    }));

    // Create app and load initial data
    let mut app = App::new(args.file, thresholds);
    let _ = app.reload_data();

    // Run the main loop
    let result = run_app(&mut terminal, &mut app, Duration::from_secs(args.refresh));

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    refresh_interval: Duration,
) -> Result<()> {
    let mut last_refresh = Instant::now();

    while app.running {
        // Draw UI
        terminal.draw(|frame| {
            let chunks = Layout::vertical([
                Constraint::Length(1), // Header bar
                Constraint::Length(1), // Tabs
                Constraint::Min(10),   // Content
                Constraint::Length(1), // Status bar
            ])
            .split(frame.area());

            // Render header with system health
            ui::common::render_header(frame, app, chunks[0]);

            // Render tabs
            ui::common::render_tabs(frame, app, chunks[1]);

            // Render current view (ModuleDetail falls back to Summary as it's now an overlay)
            match app.current_view {
                View::Summary | View::ModuleDetail => ui::summary::render(frame, app, chunks[2]),
                View::Bottleneck => ui::bottleneck::render(frame, app, chunks[2]),
                View::DataFlow => ui::flow::render(frame, app, chunks[2]),
            }

            // Render status bar
            ui::common::render_status_bar(frame, app, chunks[3]);

            // Render detail overlay if active
            if app.show_detail_overlay {
                ui::detail::render_overlay(frame, app, frame.area());
            }

            // Render help overlay if active
            if app.show_help {
                ui::common::render_help(frame, app, frame.area());
            }
        })?;

        // Poll for events with a short timeout
        if let Some(event) = events::poll_event(Duration::from_millis(100))? {
            match event {
                Event::Key(key) => events::handle_key_event(app, key),
                Event::Mouse(mouse) => {
                    // Content starts after header (1) + tabs (1) + table header (1)
                    events::handle_mouse_event(app, mouse, 3);
                }
                Event::Resize(_, _) => {
                    // Terminal will redraw on next iteration
                }
                _ => {}
            }
        }

        // Auto-refresh data periodically
        if last_refresh.elapsed() >= refresh_interval {
            let _ = app.reload_data();
            last_refresh = Instant::now();
        }
    }

    Ok(())
}

/// Export current monitor state to a JSON file
fn export_to_file(
    monitor_path: &std::path::Path,
    export_path: &std::path::Path,
    thresholds: &data::Thresholds,
) -> Result<()> {
    use std::io::Write;

    let monitor_data = data::MonitorData::load(monitor_path, thresholds)?;

    // Build export structure
    let mut export = serde_json::Map::new();

    // Summary
    let mut summary = serde_json::Map::new();
    summary.insert(
        "total_modules".to_string(),
        serde_json::json!(monitor_data.modules.len()),
    );

    let healthy =
        monitor_data.modules.iter().filter(|m| m.health == data::HealthStatus::Healthy).count();
    let warning =
        monitor_data.modules.iter().filter(|m| m.health == data::HealthStatus::Warning).count();
    let critical =
        monitor_data.modules.iter().filter(|m| m.health == data::HealthStatus::Critical).count();

    summary.insert("healthy".to_string(), serde_json::json!(healthy));
    summary.insert("warning".to_string(), serde_json::json!(warning));
    summary.insert("critical".to_string(), serde_json::json!(critical));

    let total_reads: u64 = monitor_data.modules.iter().map(|m| m.total_read).sum();
    let total_writes: u64 = monitor_data.modules.iter().map(|m| m.total_written).sum();
    summary.insert("total_reads".to_string(), serde_json::json!(total_reads));
    summary.insert("total_writes".to_string(), serde_json::json!(total_writes));

    export.insert("summary".to_string(), serde_json::Value::Object(summary));

    // Modules
    let modules: Vec<serde_json::Value> = monitor_data
        .modules
        .iter()
        .map(|m| {
            serde_json::json!({
                "name": m.name,
                "total_read": m.total_read,
                "total_written": m.total_written,
                "health": format!("{:?}", m.health),
                "reads": m.reads.iter().map(|r| {
                    serde_json::json!({
                        "topic": r.topic,
                        "read": r.read,
                        "pending_for": r.pending_for.map(|d| format!("{:?}", d)),
                        "unread": r.unread,
                        "status": format!("{:?}", r.status)
                    })
                }).collect::<Vec<_>>(),
                "writes": m.writes.iter().map(|w| {
                    serde_json::json!({
                        "topic": w.topic,
                        "written": w.written,
                        "pending_for": w.pending_for.map(|d| format!("{:?}", d)),
                        "status": format!("{:?}", w.status)
                    })
                }).collect::<Vec<_>>()
            })
        })
        .collect();
    export.insert("modules".to_string(), serde_json::Value::Array(modules));

    // Bottlenecks
    let bottlenecks: Vec<serde_json::Value> = monitor_data
        .unhealthy_topics()
        .iter()
        .map(|(module, topic)| {
            serde_json::json!({
                "module": module.name,
                "topic": topic.topic(),
                "status": format!("{:?}", topic.status()),
                "pending_for": topic.pending_for().map(|d| format!("{:?}", d))
            })
        })
        .collect();
    export.insert(
        "bottlenecks".to_string(),
        serde_json::Value::Array(bottlenecks),
    );

    // Write to file
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(export))?;
    let mut file = std::fs::File::create(export_path)?;
    file.write_all(json.as_bytes())?;

    println!("Exported monitor state to: {}", export_path.display());
    Ok(())
}
