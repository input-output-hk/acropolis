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
}

pub struct App {
    pub running: bool,
    pub current_view: View,
    pub show_help: bool,

    // Data
    pub monitor_path: PathBuf,
    pub data: Option<MonitorData>,
    pub history: History,
    pub load_error: Option<String>,
    pub thresholds: Thresholds,

    // Navigation state
    pub selected_module_index: usize,
    pub selected_topic_index: usize,

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
    pub fn new(monitor_path: PathBuf) -> Self {
        Self {
            running: true,
            current_view: View::Summary,
            show_help: false,
            monitor_path,
            data: None,
            history: History::new(),
            load_error: None,
            thresholds: Thresholds::default(),
            selected_module_index: 0,
            selected_topic_index: 0,
            sort_column: SortColumn::default(),
            sort_ascending: true,
            filter_text: String::new(),
            filter_active: false,
            theme: Theme::auto_detect(),
        }
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
        if self.current_view == View::Summary {
            self.current_view = View::ModuleDetail;
        }
    }

    pub fn go_back(&mut self) {
        if self.current_view == View::ModuleDetail {
            self.current_view = View::Summary;
        }
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
}
