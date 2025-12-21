//! Application state and navigation logic.

use anyhow::Result;

use crate::data::{History, MonitorData, Thresholds};
use crate::source::DataSource;
use crate::ui::summary::SortColumn;
use crate::ui::BottleneckSortColumn;
use crate::ui::Theme;

/// The current view/tab in the TUI.
///
/// Module detail is shown as an overlay (controlled by `App::show_detail_overlay`)
/// rather than as a separate view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Overview of all modules with health status.
    Summary,
    /// Topics with pending reads/writes that need attention.
    Bottleneck,
    /// Adjacency matrix showing producer/consumer relationships.
    DataFlow,
}

impl View {
    /// Cycle to the next view.
    pub fn next(self) -> Self {
        match self {
            View::Summary => View::Bottleneck,
            View::Bottleneck => View::DataFlow,
            View::DataFlow => View::Summary,
        }
    }

    /// Cycle to the previous view.
    pub fn prev(self) -> Self {
        match self {
            View::Summary => View::DataFlow,
            View::Bottleneck => View::Summary,
            View::DataFlow => View::Bottleneck,
        }
    }

    /// Returns the display label for this view.
    pub fn label(&self) -> &'static str {
        match self {
            View::Summary => "Summary",
            View::Bottleneck => "Bottlenecks",
            View::DataFlow => "Flow",
        }
    }
}

/// Saved state for returning to a previous view.
///
/// Used by the view stack to restore navigation state when going back.
#[derive(Debug, Clone)]
pub struct ViewState {
    /// The view that was active.
    pub view: View,
    /// The selected module index in that view.
    pub selected_module_index: usize,
    /// The selected topic index (for Bottleneck view).
    pub selected_topic_index: usize,
}

/// Main application state.
pub struct App {
    pub running: bool,
    pub current_view: View,
    pub show_help: bool,
    pub show_detail_overlay: bool,

    // Data source
    source: Box<dyn DataSource>,
    pub data: Option<MonitorData>,
    pub history: History,
    pub load_error: Option<String>,
    pub thresholds: Thresholds,

    // Navigation state
    pub selected_module_index: usize,
    pub selected_topic_index: usize,
    pub view_stack: Vec<ViewState>,

    // Sorting (Summary view)
    pub sort_column: SortColumn,
    pub sort_ascending: bool,

    // Sorting (Bottleneck view)
    pub bottleneck_sort_column: BottleneckSortColumn,
    pub bottleneck_sort_ascending: bool,

    // Search/filter
    pub filter_text: String,
    pub filter_active: bool,

    // UI
    pub theme: Theme,

    // Status message (temporary feedback)
    pub status_message: Option<(String, std::time::Instant)>,
}

impl App {
    /// Create a new App with the given data source and thresholds.
    pub fn new(source: Box<dyn DataSource>, thresholds: Thresholds) -> Self {
        Self {
            running: true,
            current_view: View::Summary,
            show_help: false,
            show_detail_overlay: false,
            source,
            data: None,
            history: History::new(),
            load_error: None,
            thresholds,
            selected_module_index: 0,
            selected_topic_index: 0,
            view_stack: Vec::new(),
            sort_column: SortColumn::default(),
            sort_ascending: true,
            bottleneck_sort_column: BottleneckSortColumn::default(),
            bottleneck_sort_ascending: false, // Default descending (critical first)
            filter_text: String::new(),
            filter_active: false,
            theme: Theme::auto_detect(),
            status_message: None,
        }
    }

    /// Returns a description of the current data source.
    pub fn source_description(&self) -> &str {
        self.source.description()
    }

    /// Set a temporary status message that will be shown for a few seconds.
    pub fn set_status_message(&mut self, message: String) {
        self.status_message = Some((message, std::time::Instant::now()));
    }

    /// Get the current status message if it hasn't expired (3 seconds).
    pub fn get_status_message(&self) -> Option<&str> {
        if let Some((msg, time)) = &self.status_message {
            if time.elapsed() < std::time::Duration::from_secs(3) {
                return Some(msg);
            }
        }
        None
    }

    /// Push current state to stack and navigate to a new view.
    #[allow(dead_code)]
    pub fn push_view(&mut self, view: View) {
        self.view_stack.push(ViewState {
            view: self.current_view,
            selected_module_index: self.selected_module_index,
            selected_topic_index: self.selected_topic_index,
        });
        self.current_view = view;
        self.selected_topic_index = 0;
    }

    /// Pop the view stack and restore previous state.
    pub fn pop_view(&mut self) -> bool {
        if let Some(state) = self.view_stack.pop() {
            self.current_view = state.view;
            self.selected_module_index = state.selected_module_index;
            self.selected_topic_index = state.selected_topic_index;
            true
        } else {
            false
        }
    }

    /// Get breadcrumb trail for current navigation.
    pub fn breadcrumb(&self) -> String {
        let mut parts: Vec<&str> = self.view_stack.iter().map(|s| s.view.label()).collect();
        parts.push(self.current_view.label());
        parts.join(" > ")
    }

    /// Poll the data source for new data.
    ///
    /// Returns Ok(true) if new data was received, Ok(false) if no new data,
    /// or Err if there was an error.
    pub fn reload_data(&mut self) -> Result<bool> {
        // Check for errors from the source
        if let Some(err) = self.source.error() {
            self.load_error = Some(err.to_string());
            return Ok(false);
        }

        // Poll for new data
        if let Some(snapshot) = self.source.poll() {
            let data = MonitorData::from_snapshot(snapshot, &self.thresholds);

            // Record history before updating
            self.history.record(&data);
            self.data = Some(data);
            self.load_error = None;

            // Clamp selection indices
            if let Some(ref data) = self.data {
                if self.selected_module_index >= data.modules.len() {
                    self.selected_module_index = data.modules.len().saturating_sub(1);
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Switch to the next view (cycles through Summary → Bottleneck → Flow).
    pub fn next_view(&mut self) {
        self.current_view = self.current_view.next();
        self.selected_topic_index = 0;
    }

    /// Switch to the previous view (cycles through Flow → Bottleneck → Summary).
    pub fn prev_view(&mut self) {
        self.current_view = self.current_view.prev();
        self.selected_topic_index = 0;
    }

    /// Switch to a specific view.
    pub fn set_view(&mut self, view: View) {
        self.current_view = view;
        self.selected_topic_index = 0;
    }

    /// Move selection down by one item.
    pub fn select_next(&mut self) {
        self.select_next_n(1);
    }

    /// Move selection up by one item.
    pub fn select_prev(&mut self) {
        self.select_prev_n(1);
    }

    /// Move selection down by n items.
    pub fn select_next_n(&mut self, n: usize) {
        match self.current_view {
            View::Summary => {
                // Navigate by visual position in filtered/sorted list
                if let Some(ref data) = self.data {
                    let filtered_count = self.filtered_module_count(data);
                    let max = filtered_count.saturating_sub(1);
                    self.selected_module_index = (self.selected_module_index + n).min(max);
                }
            }
            View::DataFlow => {
                // DataFlow doesn't use filtering, navigate by raw module count
                if let Some(ref data) = self.data {
                    let max = data.modules.len().saturating_sub(1);
                    self.selected_module_index = (self.selected_module_index + n).min(max);
                }
            }
            View::Bottleneck => {
                if let Some(ref data) = self.data {
                    let count = self.filtered_bottleneck_count(data);
                    let max = count.saturating_sub(1);
                    self.selected_topic_index = (self.selected_topic_index + n).min(max);
                }
            }
        }
    }

    /// Move selection up by n items.
    pub fn select_prev_n(&mut self, n: usize) {
        match self.current_view {
            View::Summary | View::DataFlow => {
                self.selected_module_index = self.selected_module_index.saturating_sub(n);
            }
            View::Bottleneck => {
                self.selected_topic_index = self.selected_topic_index.saturating_sub(n);
            }
        }
    }

    /// Jump to the first item in the list.
    pub fn select_first(&mut self) {
        match self.current_view {
            View::Summary | View::DataFlow => {
                self.selected_module_index = 0;
            }
            View::Bottleneck => {
                self.selected_topic_index = 0;
            }
        }
    }

    /// Jump to the last item in the list.
    pub fn select_last(&mut self) {
        match self.current_view {
            View::Summary => {
                if let Some(ref data) = self.data {
                    let filtered_count = self.filtered_module_count(data);
                    self.selected_module_index = filtered_count.saturating_sub(1);
                }
            }
            View::DataFlow => {
                if let Some(ref data) = self.data {
                    self.selected_module_index = data.modules.len().saturating_sub(1);
                }
            }
            View::Bottleneck => {
                if let Some(ref data) = self.data {
                    let count = self.filtered_bottleneck_count(data);
                    self.selected_topic_index = count.saturating_sub(1);
                }
            }
        }
    }

    /// Get count of modules after applying filter.
    fn filtered_module_count(&self, data: &MonitorData) -> usize {
        if self.filter_text.is_empty() {
            return data.modules.len();
        }
        data.modules.iter().filter(|m| self.matches_filter(&m.name)).count()
    }

    /// Get the actual module index from the visual index (after sorting/filtering).
    ///
    /// Returns the raw index into `data.modules` for the currently selected visual row.
    /// This is needed because the Summary view applies sorting and filtering, so the
    /// visual row index differs from the underlying data index.
    pub fn get_selected_module_raw_index(&self) -> Option<usize> {
        let data = self.data.as_ref()?;

        match self.current_view {
            View::Summary => {
                // Build sorted/filtered list and look up raw index
                let mut modules: Vec<(usize, &crate::data::ModuleData)> = data
                    .modules
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| self.matches_filter(&m.name))
                    .collect();
                crate::ui::summary::sort_modules_by(
                    &mut modules,
                    self.sort_column,
                    self.sort_ascending,
                );

                modules.get(self.selected_module_index).map(|(idx, _)| *idx)
            }
            View::DataFlow => {
                // DataFlow uses raw index directly (no filtering)
                if self.selected_module_index < data.modules.len() {
                    Some(self.selected_module_index)
                } else {
                    None
                }
            }
            View::Bottleneck => {
                // Bottleneck view selects topics, not modules
                None
            }
        }
    }

    /// Open the detail overlay for the currently selected module.
    pub fn enter_detail(&mut self) {
        // Toggle the detail overlay instead of changing views
        if self.current_view == View::Summary || self.current_view == View::Bottleneck {
            self.show_detail_overlay = true;
        }
    }

    /// Navigate back: close overlay first, then pop view stack, then go to Summary.
    pub fn go_back(&mut self) {
        // First close any overlays
        if self.show_detail_overlay {
            self.show_detail_overlay = false;
            return;
        }
        // Then try to pop the view stack
        if !self.pop_view() {
            // If stack is empty, go to summary
            if self.current_view != View::Summary {
                self.current_view = View::Summary;
            }
        }
    }

    /// Close the detail overlay if open.
    pub fn close_overlay(&mut self) {
        self.show_detail_overlay = false;
    }

    /// Toggle the help overlay.
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    /// Cycle to the next sort column for the current view.
    pub fn cycle_sort(&mut self) {
        match self.current_view {
            View::Summary => self.sort_column = self.sort_column.next(),
            View::Bottleneck => self.bottleneck_sort_column = self.bottleneck_sort_column.next(),
            _ => {}
        }
    }

    /// Toggle sort direction between ascending and descending.
    pub fn toggle_sort_direction(&mut self) {
        match self.current_view {
            View::Summary => self.sort_ascending = !self.sort_ascending,
            View::Bottleneck => self.bottleneck_sort_ascending = !self.bottleneck_sort_ascending,
            _ => {}
        }
    }

    /// Enter filter input mode (starts capturing keystrokes for search).
    pub fn start_filter(&mut self) {
        self.filter_active = true;
    }

    /// Exit filter input mode without clearing the filter text.
    pub fn cancel_filter(&mut self) {
        self.filter_active = false;
    }

    /// Clear the filter text and exit filter mode.
    pub fn clear_filter(&mut self) {
        self.filter_text.clear();
        self.filter_active = false;
    }

    /// Append a character to the filter text.
    pub fn filter_push(&mut self, c: char) {
        self.filter_text.push(c);
    }

    /// Remove the last character from the filter text.
    pub fn filter_pop(&mut self) {
        self.filter_text.pop();
    }

    /// Check if a module name matches the current filter.
    pub fn matches_filter(&self, name: &str) -> bool {
        if self.filter_text.is_empty() {
            return true;
        }
        name.to_lowercase().contains(&self.filter_text.to_lowercase())
    }

    /// Get count of bottlenecks after applying filter.
    fn filtered_bottleneck_count(&self, data: &MonitorData) -> usize {
        if self.filter_text.is_empty() {
            return data.unhealthy_topics().len();
        }
        let search = self.filter_text.to_lowercase();
        data.unhealthy_topics()
            .iter()
            .filter(|(module, topic)| {
                module.name.to_lowercase().contains(&search)
                    || topic.topic().to_lowercase().contains(&search)
            })
            .count()
    }

    /// Signal the application to quit.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Export current state to a file.
    pub fn export_state(&self, path: &std::path::Path) -> anyhow::Result<()> {
        use std::io::Write;

        let Some(ref data) = self.data else {
            anyhow::bail!("No data to export");
        };

        let mut export = serde_json::Map::new();

        // Summary
        let mut summary = serde_json::Map::new();
        summary.insert(
            "total_modules".to_string(),
            serde_json::json!(data.modules.len()),
        );

        let healthy =
            data.modules.iter().filter(|m| m.health == crate::data::HealthStatus::Healthy).count();
        let warning =
            data.modules.iter().filter(|m| m.health == crate::data::HealthStatus::Warning).count();
        let critical =
            data.modules.iter().filter(|m| m.health == crate::data::HealthStatus::Critical).count();

        summary.insert("healthy".to_string(), serde_json::json!(healthy));
        summary.insert("warning".to_string(), serde_json::json!(warning));
        summary.insert("critical".to_string(), serde_json::json!(critical));

        export.insert("summary".to_string(), serde_json::Value::Object(summary));

        // Modules (simplified for in-app export)
        let modules: Vec<serde_json::Value> = data
            .modules
            .iter()
            .map(|m| {
                serde_json::json!({
                    "name": m.name,
                    "total_read": m.total_read,
                    "total_written": m.total_written,
                    "health": format!("{:?}", m.health)
                })
            })
            .collect();
        export.insert("modules".to_string(), serde_json::Value::Array(modules));

        let json = serde_json::to_string_pretty(&serde_json::Value::Object(export))?;
        let mut file = std::fs::File::create(path)?;
        file.write_all(json.as_bytes())?;

        Ok(())
    }
}
