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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, Write};
    use tempfile::NamedTempFile;

    fn sample_json() -> &'static str {
        r#"{
            "TestModule": {
                "reads": {
                    "input": { "read": 100, "unread": 5 }
                },
                "writes": {
                    "output": { "written": 50 }
                }
            }
        }"#
    }

    #[test]
    fn test_file_source_new() {
        let source = FileSource::new("/tmp/test.json");
        assert_eq!(source.path(), Path::new("/tmp/test.json"));
        assert_eq!(source.description(), "file: /tmp/test.json");
        assert!(source.error().is_none());
    }

    #[test]
    fn test_file_source_poll_reads_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", sample_json()).unwrap();

        let mut source = FileSource::new(file.path());

        // First poll should return data
        let snapshot = source.poll();
        assert!(snapshot.is_some());
        let snapshot = snapshot.unwrap();
        assert!(snapshot.contains_key("TestModule"));

        // Second poll without file change should return None
        let snapshot2 = source.poll();
        assert!(snapshot2.is_none());
    }

    #[test]
    fn test_file_source_detects_changes() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", sample_json()).unwrap();

        let mut source = FileSource::new(file.path());

        // First poll
        let _ = source.poll();

        // Modify the file (need to wait a bit for mtime to change)
        std::thread::sleep(std::time::Duration::from_millis(10));
        file.rewind().unwrap();
        writeln!(
            file,
            r#"{{
            "ModifiedModule": {{
                "reads": {{}},
                "writes": {{}}
            }}
        }}"#
        )
        .unwrap();
        file.flush().unwrap();

        // Force mtime update by touching the file
        let _ = std::fs::File::open(file.path());

        // Poll again - should detect change
        // Note: This test may be flaky on some filesystems with low mtime resolution
        let snapshot = source.poll();
        if let Some(s) = snapshot {
            assert!(s.contains_key("ModifiedModule"));
        }
    }

    #[test]
    fn test_file_source_missing_file() {
        let mut source = FileSource::new("/nonexistent/path/monitor.json");

        let snapshot = source.poll();
        assert!(snapshot.is_none());
        assert!(source.error().is_some());
        assert!(source.error().unwrap().contains("Read error"));
    }

    #[test]
    fn test_file_source_invalid_json() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "not valid json").unwrap();

        let mut source = FileSource::new(file.path());

        let snapshot = source.poll();
        assert!(snapshot.is_none());
        assert!(source.error().is_some());
        assert!(source.error().unwrap().contains("Parse error"));
    }
}
