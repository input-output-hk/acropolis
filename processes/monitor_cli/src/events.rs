use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

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

    // If detail overlay is shown, handle overlay-specific keys
    if app.show_detail_overlay {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace | KeyCode::Char('q') => {
                app.close_overlay();
            }
            _ => {}
        }
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

        // View switching (now uses push_view for history)
        KeyCode::Tab => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_view();
            } else {
                app.next_view();
            }
        }
        KeyCode::BackTab => app.prev_view(),

        // Direct view access (Detail is now overlay-only, accessed via Enter)
        KeyCode::Char('1') => app.set_view(View::Summary),
        KeyCode::Char('2') => app.set_view(View::Bottleneck),
        KeyCode::Char('3') => app.set_view(View::DataFlow),

        // Navigation
        KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),

        // Enter detail overlay
        KeyCode::Enter => app.enter_detail(),

        // Go back (Esc and Backspace)
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

        // Export
        KeyCode::Char('e') => {
            let export_path = std::path::PathBuf::from("monitor_export.json");
            let _ = app.export_state(&export_path);
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

/// Handle mouse events
pub fn handle_mouse_event(app: &mut App, mouse: MouseEvent, content_start_row: u16) {
    match mouse.kind {
        // Scroll wheel
        MouseEventKind::ScrollUp => {
            app.select_prev();
        }
        MouseEventKind::ScrollDown => {
            app.select_next();
        }

        // Click to select
        MouseEventKind::Down(MouseButton::Left) => {
            // Calculate which row was clicked (accounting for header/tabs)
            let clicked_row = mouse.row;

            // Check if clicking in content area (after header, tabs, table header)
            if clicked_row > content_start_row {
                let item_row = (clicked_row - content_start_row - 1) as usize;

                match app.current_view {
                    View::Summary => {
                        if let Some(ref data) = app.data {
                            // Account for filtered modules
                            let filtered_count =
                                data.modules.iter().filter(|m| app.matches_filter(&m.name)).count();
                            if item_row < filtered_count {
                                app.selected_module_index = item_row;
                            }
                        }
                    }
                    View::Bottleneck => {
                        if let Some(ref data) = app.data {
                            let count = data.unhealthy_topics().len();
                            if item_row < count {
                                app.selected_topic_index = item_row;
                            }
                        }
                    }
                    View::DataFlow => {
                        if let Some(ref data) = app.data {
                            let graph = crate::data::DataFlowGraph::from_monitor_data(data);
                            if item_row < graph.topics.len() {
                                app.selected_topic_index = item_row;
                            }
                        }
                    }
                    View::ModuleDetail => {
                        // Module detail doesn't have selectable items currently
                    }
                }
            }

            // Check for tab clicks (row 1, after header)
            if clicked_row == 1 {
                let col = mouse.column;
                // Approximate tab positions: Summary (0-12), Bottlenecks (13-28), Flow (29-38)
                if col < 13 {
                    app.set_view(View::Summary);
                } else if col < 29 {
                    app.set_view(View::Bottleneck);
                } else if col < 39 {
                    app.set_view(View::DataFlow);
                }
            }
        }

        // Double-click to enter detail
        MouseEventKind::Down(MouseButton::Right) => {
            // Right-click goes back
            app.go_back();
        }

        _ => {}
    }
}
