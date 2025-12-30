//! Operational certificate counters reader for snapshot bootstrap.
//!
//! Reads OpCerts.csv containing pool operational certificate counter values
//! needed for KES signature validation during bootstrap.

// TODO: Remove this once the module is integrated into the bootstrap flow
#![allow(dead_code)]

use acropolis_common::serialization::Bech32Conversion;
use acropolis_common::PoolId;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OpCertsError {
    #[error("Failed to read {0}: {1}")]
    ReadFile(PathBuf, std::io::Error),

    #[error("Failed to parse CSV at {path}, line {line}: {reason}")]
    Parse {
        path: PathBuf,
        line: usize,
        reason: String,
    },

    #[error("Invalid pool ID at {path}, line {line}: {pool_id}")]
    InvalidPoolId {
        path: PathBuf,
        line: usize,
        pool_id: String,
    },

    #[error("Invalid counter value at {path}, line {line}: {value}")]
    InvalidCounter {
        path: PathBuf,
        line: usize,
        value: String,
    },
}

/// Operational certificate counters loaded from CSV.
#[derive(Debug, Clone, Default)]
pub struct OpCertsContext {
    /// Map of pool ID to latest operational certificate counter
    pub counters: HashMap<PoolId, u64>,
}

impl OpCertsContext {
    /// Path to the OpCerts.csv file in the network directory.
    pub fn path(network_dir: &Path) -> PathBuf {
        network_dir.join("OpCerts.csv")
    }

    /// Load operational certificate counters from CSV file.
    ///
    /// The CSV format is:
    /// ```csv
    /// "pool_id","latest_op_cert_counter"
    /// "pool1abc...","3"
    /// ```
    pub fn load(network_dir: &Path) -> Result<Self, OpCertsError> {
        let path = Self::path(network_dir);

        // If file doesn't exist, return empty counters (graceful degradation)
        if !path.exists() {
            tracing::warn!(
                "OpCerts.csv not found at {}, starting with empty counters",
                path.display()
            );
            return Ok(Self::default());
        }

        let content =
            fs::read_to_string(&path).map_err(|e| OpCertsError::ReadFile(path.clone(), e))?;

        Self::parse(&path, &content)
    }

    /// Parse CSV content into OpCertsContext.
    fn parse(path: &Path, content: &str) -> Result<Self, OpCertsError> {
        let mut counters = HashMap::new();

        for (line_num, line) in content.lines().enumerate() {
            // Skip header line
            if line_num == 0 {
                continue;
            }

            // Skip empty lines
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Parse CSV line: "pool_id","counter"
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() != 2 {
                return Err(OpCertsError::Parse {
                    path: path.to_path_buf(),
                    line: line_num + 1,
                    reason: format!("Expected 2 columns, found {}", parts.len()),
                });
            }

            // Remove quotes from pool_id
            let pool_id_str = parts[0].trim().trim_matches('"');

            // Parse pool ID (bech32 format: pool1...)
            let pool_id =
                PoolId::from_bech32(pool_id_str).map_err(|_| OpCertsError::InvalidPoolId {
                    path: path.to_path_buf(),
                    line: line_num + 1,
                    pool_id: pool_id_str.to_string(),
                })?;

            // Remove quotes from counter and parse
            let counter_str = parts[1].trim().trim_matches('"');
            let counter = counter_str.parse::<u64>().map_err(|_| OpCertsError::InvalidCounter {
                path: path.to_path_buf(),
                line: line_num + 1,
                value: counter_str.to_string(),
            })?;

            counters.insert(pool_id, counter);
        }

        tracing::info!(
            "Loaded {} operational certificate counters from {}",
            counters.len(),
            path.display()
        );

        Ok(Self { counters })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn load_returns_empty_when_file_missing() {
        let temp_dir = TempDir::new().unwrap();

        let result = OpCertsContext::load(temp_dir.path()).unwrap();

        assert!(result.counters.is_empty());
    }

    #[test]
    fn load_parses_valid_csv() {
        let temp_dir = TempDir::new().unwrap();
        let csv_content = r#""pool_id","latest_op_cert_counter"
"pool1026f3pyjhxjafmtjyyny5ytuh2kraxakfa8eu5h844uv2upzxvq","3"
"pool102vsulhfx8ua2j9fwl2u7gv57fhhutc3tp6juzaefgrn7ae35wm","14"
"#;
        fs::write(OpCertsContext::path(temp_dir.path()), csv_content).unwrap();

        let result = OpCertsContext::load(temp_dir.path()).unwrap();

        assert_eq!(result.counters.len(), 2);
    }

    #[test]
    fn load_fails_for_invalid_pool_id() {
        let temp_dir = TempDir::new().unwrap();
        let csv_content = r#""pool_id","latest_op_cert_counter"
"invalid_pool_id","5"
"#;
        fs::write(OpCertsContext::path(temp_dir.path()), csv_content).unwrap();

        let err = OpCertsContext::load(temp_dir.path()).unwrap_err();

        assert!(matches!(err, OpCertsError::InvalidPoolId { .. }));
    }

    #[test]
    fn load_fails_for_invalid_counter() {
        let temp_dir = TempDir::new().unwrap();
        let csv_content = r#""pool_id","latest_op_cert_counter"
"pool1026f3pyjhxjafmtjyyny5ytuh2kraxakfa8eu5h844uv2upzxvq","not_a_number"
"#;
        fs::write(OpCertsContext::path(temp_dir.path()), csv_content).unwrap();

        let err = OpCertsContext::load(temp_dir.path()).unwrap_err();

        assert!(matches!(err, OpCertsError::InvalidCounter { .. }));
    }

    #[test]
    fn load_fails_for_wrong_column_count() {
        let temp_dir = TempDir::new().unwrap();
        let csv_content = r#""pool_id","latest_op_cert_counter"
"pool1026f3pyjhxjafmtjyyny5ytuh2kraxakfa8eu5h844uv2upzxvq"
"#;
        fs::write(OpCertsContext::path(temp_dir.path()), csv_content).unwrap();

        let err = OpCertsContext::load(temp_dir.path()).unwrap_err();

        assert!(matches!(err, OpCertsError::Parse { .. }));
    }

    #[test]
    fn load_skips_empty_lines() {
        let temp_dir = TempDir::new().unwrap();
        let csv_content = r#""pool_id","latest_op_cert_counter"

"pool1026f3pyjhxjafmtjyyny5ytuh2kraxakfa8eu5h844uv2upzxvq","5"

"#;
        fs::write(OpCertsContext::path(temp_dir.path()), csv_content).unwrap();

        let result = OpCertsContext::load(temp_dir.path()).unwrap();

        assert_eq!(result.counters.len(), 1);
    }
}
