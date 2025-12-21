// Binary includes library modules - some public API items are only for library consumers
#![allow(unused)]

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
mod source;
mod ui;

#[cfg(feature = "subscribe")]
mod subscribe;

use app::{App, View};
use source::{DataSource, FileSource, StreamSource};

#[derive(Parser, Debug)]
#[command(name = "caryatid-doctor")]
#[command(about = "Diagnostic TUI for monitoring Caryatid message bus activity")]
struct Args {
    /// Path to monitor.json file
    #[cfg_attr(
        feature = "subscribe",
        arg(short, long, default_value = "monitor.json", conflicts_with_all = ["connect", "subscribe"])
    )]
    #[cfg_attr(
        not(feature = "subscribe"),
        arg(short, long, default_value = "monitor.json", conflicts_with_all = ["connect"])
    )]
    file: PathBuf,

    /// Connect to a TCP endpoint for live snapshots (host:port)
    #[cfg_attr(
        feature = "subscribe",
        arg(short, long, conflicts_with_all = ["file", "subscribe"])
    )]
    #[cfg_attr(
        not(feature = "subscribe"),
        arg(short, long, conflicts_with_all = ["file"])
    )]
    connect: Option<String>,

    /// Subscribe to monitor snapshots via caryatid message bus.
    /// Requires a config file path (for message bus connection settings).
    /// Use with --topic to specify the subscription topic.
    #[cfg(feature = "subscribe")]
    #[arg(short, long, conflicts_with_all = ["file", "connect"])]
    subscribe: Option<PathBuf>,

    /// Topic to subscribe to (used with --subscribe)
    #[cfg(feature = "subscribe")]
    #[arg(long, default_value = "caryatid.monitor", requires = "subscribe")]
    topic: String,

    /// Refresh interval in seconds (only used with --file)
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
    #[arg(short, long, conflicts_with_all = ["connect", "subscribe"])]
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

    // Handle TCP connection mode
    if let Some(ref addr) = args.connect {
        return run_with_tcp(addr, thresholds);
    }

    // Handle subscribe mode (caryatid message bus)
    #[cfg(feature = "subscribe")]
    if let Some(ref config_path) = args.subscribe {
        return run_with_subscribe(config_path, &args.topic, thresholds);
    }

    // Default: file-based mode
    run_with_file(&args.file, thresholds, Duration::from_secs(args.refresh))
}

/// Run with a file-based data source
fn run_with_file(path: &PathBuf, thresholds: data::Thresholds, refresh: Duration) -> Result<()> {
    let source = Box::new(FileSource::new(path));
    run_tui(source, thresholds, refresh)
}

/// Run with a caryatid message bus subscription
#[cfg(feature = "subscribe")]
fn run_with_subscribe(
    config_path: &std::path::Path,
    topic: &str,
    thresholds: data::Thresholds,
) -> Result<()> {
    use subscribe::create_subscriber;

    // Build a tokio runtime
    let rt = tokio::runtime::Runtime::new()?;

    // Create the subscriber and get the channel source
    let (source, handle) = rt.block_on(async {
        let (source, handle) = create_subscriber(config_path, topic).await?;
        Ok::<_, anyhow::Error>((source, handle))
    })?;

    // Run the TUI in the main thread while the async runtime runs in the background
    let result = run_tui(Box::new(source), thresholds, Duration::from_millis(100));

    // Signal shutdown
    handle.abort();

    result
}

/// Run with a TCP stream data source
fn run_with_tcp(addr: &str, thresholds: data::Thresholds) -> Result<()> {
    // Build a tokio runtime for the TCP connection
    let rt = tokio::runtime::Runtime::new()?;

    let source = rt.block_on(async {
        use tokio::net::TcpStream;

        println!("Connecting to {}...", addr);
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                println!("Connected!");
                Ok(Box::new(StreamSource::spawn(stream, addr)) as Box<dyn DataSource>)
            }
            Err(e) => Err(anyhow::anyhow!("Failed to connect to {}: {}", addr, e)),
        }
    })?;

    // For TCP, we poll continuously (no refresh interval needed)
    run_tui(source, thresholds, Duration::from_millis(100))
}

/// Run the TUI with the given data source
fn run_tui(
    source: Box<dyn DataSource>,
    thresholds: data::Thresholds,
    refresh_interval: Duration,
) -> Result<()> {
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
    let mut app = App::new(source, thresholds);
    let _ = app.reload_data();

    // Run the main loop
    let result = run_app(&mut terminal, &mut app, refresh_interval);

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

    // Minimum terminal size for usable display
    const MIN_WIDTH: u16 = 60;
    const MIN_HEIGHT: u16 = 12;

    while app.running {
        // Draw UI
        terminal.draw(|frame| {
            let area = frame.area();

            // Check for minimum terminal size
            if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
                let msg = format!(
                    "Terminal too small: {}x{}\nMinimum: {}x{}\n\nResize to continue",
                    area.width, area.height, MIN_WIDTH, MIN_HEIGHT
                );
                let paragraph = ratatui::widgets::Paragraph::new(msg)
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow));
                let centered = ratatui::layout::Rect::new(0, area.height / 2 - 2, area.width, 5);
                frame.render_widget(paragraph, centered);
                return;
            }

            let chunks = Layout::vertical([
                Constraint::Length(1), // Header bar
                Constraint::Length(1), // Tabs
                Constraint::Min(8),    // Content (reduced from 10)
                Constraint::Length(1), // Status bar
            ])
            .split(area);

            // Render header with system health
            ui::common::render_header(frame, app, chunks[0]);

            // Render tabs
            ui::common::render_tabs(frame, app, chunks[1]);

            // Render current view
            match app.current_view {
                View::Summary => ui::summary::render(frame, app, chunks[2]),
                View::Bottleneck => ui::bottleneck::render(frame, app, chunks[2]),
                View::DataFlow => ui::flow::render(frame, app, chunks[2]),
            }

            // Render status bar
            ui::common::render_status_bar(frame, app, chunks[3]);

            // Render detail overlay if active
            if app.show_detail_overlay {
                ui::detail::render_overlay(frame, app, area);
            }

            // Render help overlay if active
            if app.show_help {
                ui::common::render_help(frame, app, area);
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
