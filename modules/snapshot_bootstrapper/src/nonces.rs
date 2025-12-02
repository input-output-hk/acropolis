use acropolis_common::protocol_params::{Nonce, Nonces};
use acropolis_common::{BlockHash, Point};
use serde::{Deserialize, Deserializer};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NoncesError {
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

fn deserialize_point<'de, D>(deserializer: D) -> Result<Point, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.split_once('.')
        .and_then(|(slot_str, hash_str)| {
            Some(Point::Specific {
                slot: slot_str.parse().ok()?,
                hash: hash_str.parse().ok()?,
            })
        })
        .ok_or_else(|| serde::de::Error::custom("invalid point format"))
}

#[derive(Debug, Deserialize)]
pub struct NoncesFile {
    #[serde(deserialize_with = "deserialize_point")]
    pub at: Point,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub active: Nonce,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub candidate: Nonce,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub evolving: Nonce,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub tail: Nonce,
}

impl NoncesFile {
    pub fn path(network_dir: &Path) -> PathBuf {
        network_dir.join("nonces.json")
    }

    pub fn load(network_dir: &Path) -> Result<Self, NoncesError> {
        let path = Self::path(network_dir);
        let content =
            fs::read_to_string(&path).map_err(|e| NoncesError::ReadFile(path.clone(), e))?;
        serde_json::from_str(&content).map_err(|e| NoncesError::Parse(path, e))
    }

    pub fn into_nonces(self, epoch: u64, lab_hash: BlockHash) -> Nonces {
        Nonces {
            epoch,
            active: self.active,
            evolving: self.evolving,
            candidate: self.candidate,
            lab: Nonce::from(lab_hash),
            prev_lab: self.tail,
        }
    }
}
