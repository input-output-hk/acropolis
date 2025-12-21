//! File-based data source.
//!
//! Polls a JSON file for monitor snapshots.

use std::fs;
use std::path::{Path, PathBuf};

use super::{DataSource, MonitorSnapshot};

/// A data source that reads monitor snapshots from a JSON file.
///
/// This is the traditional mode of operation where caryatid's Monitor
/// writes snapshots to a file, and this source polls that file.
#[derive(Debug)]
pub struct FileSource {
    path: PathBuf,
    description: String,
    last_error: Option<String>,
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
        }
    }

    /// Returns the path being monitored.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl DataSource for FileSource {
    fn poll(&mut self) -> Option<MonitorSnapshot> {
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

    fn description(&self) -> &str {
        &self.description
    }

    fn error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}
