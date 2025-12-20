use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::Event,
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
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
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
    let mut app = App::new(args.file);
    let _ = app.reload_data();

    // Run the main loop
    let result = run_app(&mut terminal, &mut app, Duration::from_secs(args.refresh));

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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
                Constraint::Length(1), // Tabs
                Constraint::Min(10),   // Content
                Constraint::Length(1), // Status bar
            ])
            .split(frame.area());

            // Render tabs
            ui::common::render_tabs(frame, app, chunks[0]);

            // Render current view
            match app.current_view {
                View::Summary => ui::summary::render(frame, app, chunks[1]),
                View::Bottleneck => ui::bottleneck::render(frame, app, chunks[1]),
                View::ModuleDetail => ui::detail::render(frame, app, chunks[1]),
                View::DataFlow => ui::flow::render(frame, app, chunks[1]),
            }

            // Render status bar
            ui::common::render_status_bar(frame, app, chunks[2]);

            // Render help overlay if active
            if app.show_help {
                ui::common::render_help(frame, app, frame.area());
            }
        })?;

        // Poll for events with a short timeout
        if let Some(event) = events::poll_event(Duration::from_millis(100))? {
            match event {
                Event::Key(key) => events::handle_key_event(app, key),
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
