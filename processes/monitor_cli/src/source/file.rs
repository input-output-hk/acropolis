//! File-based data source.
//!
//! Polls a JSON file for monitor snapshots.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{DataSource, MonitorSnapshot};

/// A data source that reads monitor snapshots from a JSON file.
///
/// This is the traditional mode of operation where caryatid's Monitor
/// writes snapshots to a file, and this source polls that file.
///
/// The source tracks the file's modification time and only returns
/// new data when the file has been updated.
#[derive(Debug)]
pub struct FileSource {
    path: PathBuf,
    description: String,
    last_error: Option<String>,
    last_modified: Option<SystemTime>,
    /// Cached snapshot to return on first poll
    cached_snapshot: Option<MonitorSnapshot>,
}

impl FileSource {
    /// Create a new file source for the given path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref().to_path_buf();
        let description = format!("file: {}", path.display());
        Self {
            path,
            description,
            last_error: None,
            last_modified: None,
            cached_snapshot: None,
        }
    }

    /// Returns the path being monitored.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the file's modification time.
    fn get_modified_time(&self) -> Option<SystemTime> {
        fs::metadata(&self.path).ok()?.modified().ok()
    }

    /// Read and parse the file.
    fn read_file(&mut self) -> Option<MonitorSnapshot> {
        match fs::read_to_string(&self.path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(snapshot) => {
                    self.last_error = None;
                    Some(snapshot)
                }
                Err(e) => {
                    self.last_error = Some(format!("Parse error: {}", e));
                    None
                }
            },
            Err(e) => {
                self.last_error = Some(format!("Read error: {}", e));
                None
            }
        }
    }
}

impl DataSource for FileSource {
    fn poll(&mut self) -> Option<MonitorSnapshot> {
        let current_modified = self.get_modified_time();

        // Check if file has been modified since last read
        let file_changed = match (&self.last_modified, &current_modified) {
            (None, _) => true,        // First poll, always read
            (Some(_), None) => false, // File disappeared, don't update
            (Some(last), Some(current)) => current > last,
        };

        if file_changed {
            if let Some(snapshot) = self.read_file() {
                self.last_modified = current_modified;
                self.cached_snapshot = Some(snapshot.clone());
                return Some(snapshot);
            }
        }

        None
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}
