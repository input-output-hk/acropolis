use acropolis_common::protocol_params::{Nonce, Nonces};
use acropolis_common::BlockHash;
use serde::{Deserialize, Deserializer};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NonceContextError {
    #[error("Failed to read {0}: {1}")]
    ReadFile(PathBuf, std::io::Error),

    #[error("Failed to parse {0}: {1}")]
    Parse(PathBuf, serde_json::Error),
}

fn deserialize_nonce<'de, D>(deserializer: D) -> Result<Nonce, D::Error>
where
    D: Deserializer<'de>,
{
    let hash: BlockHash = Deserialize::deserialize(deserializer)?;
    Ok(Nonce::from(hash))
}

#[derive(Debug, Deserialize)]
pub struct NonceContext {
    pub slot: u64,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub active: Nonce,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub candidate: Nonce,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub evolving: Nonce,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub lab: Nonce,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub prev_lab: Nonce,
}

impl NonceContext {
    pub fn path(network_dir: &Path) -> PathBuf {
        network_dir.join("nonces.json")
    }

    pub fn load(network_dir: &Path) -> Result<Self, NonceContextError> {
        let path = Self::path(network_dir);
        let content =
            fs::read_to_string(&path).map_err(|e| NonceContextError::ReadFile(path.clone(), e))?;
        serde_json::from_str(&content).map_err(|e| NonceContextError::Parse(path, e))
    }

    pub fn into_nonces(self, epoch: u64) -> Nonces {
        Nonces {
            epoch,
            active: self.active,
            evolving: self.evolving,
            candidate: self.candidate,
            lab: self.lab,
            prev_lab: self.prev_lab,
        }
    }
}

#[cfg(test)]
mod nonces_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn valid_json_with_point(point: &str) -> String {
        format!(
            r#"{{
                "slot": "{point}",
                "active": "{ZERO_HASH}",
                "candidate": "{ZERO_HASH}",
                "evolving": "{ZERO_HASH}",
                "lab": "{ZERO_HASH}"
                "prev_lab": "{ZERO_HASH}"
            }}"#
        )
    }

    #[test]
    fn load_fails_when_file_missing() {
        let temp_dir = TempDir::new().unwrap();

        let err = NonceContext::load(temp_dir.path()).unwrap_err();

        assert!(matches!(err, NonceContextError::ReadFile(_, _)));
        assert!(err.to_string().contains("nonces.json"));
    }

    #[test]
    fn load_fails_for_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(NonceContext::path(temp_dir.path()), "not valid json {{{").unwrap();

        let err = NonceContext::load(temp_dir.path()).unwrap_err();

        assert!(matches!(err, NonceContextError::Parse(_, _)));
    }

    #[test]
    fn load_fails_when_missing_required_fields() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(NonceContext::path(temp_dir.path()), r#"{"at": "123.abc"}"#).unwrap();

        let err = NonceContext::load(temp_dir.path()).unwrap_err();

        assert!(matches!(err, NonceContextError::Parse(_, _)));
    }

    #[test]
    fn load_fails_for_invalid_point_format() {
        let temp_dir = TempDir::new().unwrap();

        let bad_case = format!("not_a_number.{ZERO_HASH}").clone();
        let cases = ["no_dot_separator", bad_case.as_str()];

        for invalid_point in cases {
            fs::write(
                NonceContext::path(temp_dir.path()),
                valid_json_with_point(invalid_point),
            )
            .unwrap();

            let err = NonceContext::load(temp_dir.path()).unwrap_err();
            assert!(matches!(err, NonceContextError::Parse(_, _)));
        }
    }
}
