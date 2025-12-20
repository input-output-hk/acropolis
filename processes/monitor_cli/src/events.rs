use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, View};

/// Poll for events with a timeout
pub fn poll_event(timeout: Duration) -> Result<Option<Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}

/// Handle a key event
pub fn handle_key_event(app: &mut App, key: KeyEvent) {
    // If help is shown, any key closes it
    if app.show_help {
        app.show_help = false;
        return;
    }

    // If filter input is active, handle text input
    if app.filter_active {
        handle_filter_input(app, key);
        return;
    }

    match key.code {
        // Quit
        KeyCode::Char('q') => app.quit(),

        // View switching
        KeyCode::Tab => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_view();
            } else {
                app.next_view();
            }
        }
        KeyCode::BackTab => app.prev_view(),

        // Direct view access
        KeyCode::Char('1') => app.set_view(View::Summary),
        KeyCode::Char('2') => app.set_view(View::Bottleneck),
        KeyCode::Char('3') => app.set_view(View::ModuleDetail),
        KeyCode::Char('4') => app.set_view(View::DataFlow),

        // Navigation
        KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),

        // Enter detail view
        KeyCode::Enter => app.enter_detail(),

        // Go back
        KeyCode::Esc | KeyCode::Backspace => app.go_back(),

        // Reload
        KeyCode::Char('r') => {
            let _ = app.reload_data();
        }

        // Help
        KeyCode::Char('?') => app.toggle_help(),

        // Sorting (Summary view)
        KeyCode::Char('s') => {
            if app.current_view == View::Summary {
                app.cycle_sort();
            }
        }
        KeyCode::Char('S') => {
            if app.current_view == View::Summary {
                app.toggle_sort_direction();
            }
        }

        // Filter (start typing to filter)
        KeyCode::Char('/') => app.start_filter(),

        // Clear filter
        KeyCode::Char('c') => {
            if !app.filter_text.is_empty() {
                app.clear_filter();
            }
        }

        _ => {}
    }
}

/// Handle key input while filter is active
fn handle_filter_input(app: &mut App, key: KeyEvent) {
    match key.code {
        // Confirm filter
        KeyCode::Enter => {
            app.filter_active = false;
        }

        // Cancel filter (keep text but exit input mode)
        KeyCode::Esc => {
            app.cancel_filter();
        }

        // Clear and exit
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.clear_filter();
        }

        // Backspace
        KeyCode::Backspace => {
            app.filter_pop();
            if app.filter_text.is_empty() {
                app.filter_active = false;
            }
        }

        // Type characters
        KeyCode::Char(c) => {
            app.filter_push(c);
        }

        _ => {}
    }
}
