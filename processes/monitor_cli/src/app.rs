use std::path::PathBuf;

use anyhow::Result;

use crate::data::{History, MonitorData, Thresholds};
use crate::ui::summary::SortColumn;
use crate::ui::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Summary,
    Bottleneck,
    ModuleDetail,
    DataFlow,
}

impl View {
    pub fn next(self) -> Self {
        match self {
            View::Summary => View::Bottleneck,
            View::Bottleneck => View::ModuleDetail,
            View::ModuleDetail => View::DataFlow,
            View::DataFlow => View::Summary,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            View::Summary => View::DataFlow,
            View::Bottleneck => View::Summary,
            View::ModuleDetail => View::Bottleneck,
            View::DataFlow => View::ModuleDetail,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            View::Summary => "Summary",
            View::Bottleneck => "Bottlenecks",
            View::ModuleDetail => "Detail",
            View::DataFlow => "Flow",
        }
    }
}

/// Saved state for returning to a previous view
#[derive(Debug, Clone)]
pub struct ViewState {
    pub view: View,
    pub selected_module_index: usize,
    pub selected_topic_index: usize,
}

pub struct App {
    pub running: bool,
    pub current_view: View,
    pub show_help: bool,
    pub show_detail_overlay: bool,

    // Data
    pub monitor_path: PathBuf,
    pub data: Option<MonitorData>,
    pub history: History,
    pub load_error: Option<String>,
    pub thresholds: Thresholds,

    // Navigation state
    pub selected_module_index: usize,
    pub selected_topic_index: usize,
    pub view_stack: Vec<ViewState>,

    // Sorting
    pub sort_column: SortColumn,
    pub sort_ascending: bool,

    // Search/filter
    pub filter_text: String,
    pub filter_active: bool,

    // UI
    pub theme: Theme,
}

impl App {
    pub fn new(monitor_path: PathBuf, thresholds: Thresholds) -> Self {
        Self {
            running: true,
            current_view: View::Summary,
            show_help: false,
            show_detail_overlay: false,
            monitor_path,
            data: None,
            history: History::new(),
            load_error: None,
            thresholds,
            selected_module_index: 0,
            selected_topic_index: 0,
            view_stack: Vec::new(),
            sort_column: SortColumn::default(),
            sort_ascending: true,
            filter_text: String::new(),
            filter_active: false,
            theme: Theme::auto_detect(),
        }
    }

    /// Push current state to stack and navigate to a new view
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

    /// Pop the view stack and restore previous state
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

    /// Get breadcrumb trail for current navigation
    pub fn breadcrumb(&self) -> String {
        let mut parts: Vec<&str> = self.view_stack.iter().map(|s| s.view.label()).collect();
        parts.push(self.current_view.label());
        parts.join(" > ")
    }

    /// Load or reload the monitor data
    pub fn reload_data(&mut self) -> Result<()> {
        match MonitorData::load(&self.monitor_path, &self.thresholds) {
            Ok(data) => {
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
                Ok(())
            }
            Err(e) => {
                self.load_error = Some(e.to_string());
                Err(e)
            }
        }
    }

    pub fn next_view(&mut self) {
        self.current_view = self.current_view.next();
        self.selected_topic_index = 0;
    }

    pub fn prev_view(&mut self) {
        self.current_view = self.current_view.prev();
        self.selected_topic_index = 0;
    }

    pub fn set_view(&mut self, view: View) {
        self.current_view = view;
        self.selected_topic_index = 0;
    }

    pub fn select_next(&mut self) {
        match self.current_view {
            View::Summary | View::ModuleDetail => {
                if let Some(ref data) = self.data {
                    if self.selected_module_index < data.modules.len().saturating_sub(1) {
                        self.selected_module_index += 1;
                    }
                }
            }
            View::Bottleneck => {
                if let Some(ref data) = self.data {
                    let count = data.unhealthy_topics().len();
                    if self.selected_topic_index < count.saturating_sub(1) {
                        self.selected_topic_index += 1;
                    }
                }
            }
            View::DataFlow => {
                if let Some(ref data) = self.data {
                    let graph = crate::data::DataFlowGraph::from_monitor_data(data);
                    if self.selected_topic_index < graph.topics.len().saturating_sub(1) {
                        self.selected_topic_index += 1;
                    }
                }
            }
        }
    }

    pub fn select_prev(&mut self) {
        match self.current_view {
            View::Summary | View::ModuleDetail => {
                if self.selected_module_index > 0 {
                    self.selected_module_index -= 1;
                }
            }
            View::Bottleneck | View::DataFlow => {
                if self.selected_topic_index > 0 {
                    self.selected_topic_index -= 1;
                }
            }
        }
    }

    pub fn enter_detail(&mut self) {
        // Toggle the detail overlay instead of changing views
        if self.current_view == View::Summary || self.current_view == View::Bottleneck {
            self.show_detail_overlay = true;
        }
    }

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

    pub fn close_overlay(&mut self) {
        self.show_detail_overlay = false;
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn cycle_sort(&mut self) {
        self.sort_column = self.sort_column.next();
    }

    pub fn toggle_sort_direction(&mut self) {
        self.sort_ascending = !self.sort_ascending;
    }

    pub fn start_filter(&mut self) {
        self.filter_active = true;
    }

    pub fn cancel_filter(&mut self) {
        self.filter_active = false;
    }

    pub fn clear_filter(&mut self) {
        self.filter_text.clear();
        self.filter_active = false;
    }

    pub fn filter_push(&mut self, c: char) {
        self.filter_text.push(c);
    }

    pub fn filter_pop(&mut self) {
        self.filter_text.pop();
    }

    /// Check if a module name matches the current filter
    pub fn matches_filter(&self, name: &str) -> bool {
        if self.filter_text.is_empty() {
            return true;
        }
        name.to_lowercase().contains(&self.filter_text.to_lowercase())
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Export current state to a file
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
