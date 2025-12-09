use std::{collections::HashMap, future::Future, path::Path};

use acropolis_common::Point;
use anyhow::Result;
use fjall::{Config, Keyspace, Partition, PartitionCreateOptions};
use tokio::sync::Mutex;
use tracing::warn;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CursorEntry {
    pub tip: Point,
    pub halted: bool,
}

pub trait CursorStore: Send + Sync + 'static {
    fn load(&self) -> impl Future<Output = Result<HashMap<String, CursorEntry>>> + Send;
    fn save(
        &self,
        entries: &HashMap<String, CursorEntry>,
    ) -> impl Future<Output = Result<(), CursorSaveError>> + Send;
}

// In memory cursor store (Not persisted across runs)
pub struct InMemoryCursorStore {
    entries: Mutex<HashMap<String, CursorEntry>>,
}
impl InMemoryCursorStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}
impl Default for InMemoryCursorStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorStore for InMemoryCursorStore {
    async fn load(&self) -> Result<HashMap<String, CursorEntry>> {
        let guard = self.entries.lock().await;
        Ok(guard.clone())
    }

    async fn save(&self, entries: &HashMap<String, CursorEntry>) -> Result<(), CursorSaveError> {
        let mut guard = self.entries.lock().await;
        *guard = entries.clone();
        Ok(())
    }
}

// Fjall backed cursor store (Retains last stored point)
const CURSOR_PREFIX: &str = "cursor/";

pub struct FjallCursorStore {
    partition: Partition,
}

impl FjallCursorStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let cfg = Config::new(path);
        let keyspace = Keyspace::open(cfg)?;
        let partition = keyspace.open_partition("cursor", PartitionCreateOptions::default())?;

        Ok(Self { partition })
    }

    fn key_for(name: &str) -> String {
        format!("{CURSOR_PREFIX}{name}")
    }

    fn name_from_key(key: &[u8]) -> Option<String> {
        let s = std::str::from_utf8(key).ok()?;
        s.strip_prefix(CURSOR_PREFIX).map(|n| n.to_string())
    }

    fn prefix_iter(
        &self,
    ) -> impl Iterator<Item = fjall::Result<(fjall::Slice, fjall::Slice)>> + '_ {
        self.partition.prefix(CURSOR_PREFIX)
    }
}

impl CursorStore for FjallCursorStore {
    async fn load(&self) -> Result<HashMap<String, CursorEntry>> {
        let mut out = HashMap::new();
        for next in self.prefix_iter() {
            let (key_bytes, val_bytes) = match next {
                Ok(r) => r,
                Err(e) => {
                    warn!("CursorStore: failed to read row: {:#}", e);
                    continue;
                }
            };

            let Some(name) = Self::name_from_key(&key_bytes) else {
                warn!("CursorStore: invalid or non-matching key");
                continue;
            };

            let point = match bincode::deserialize::<CursorEntry>(&val_bytes) {
                Ok(p) => p,
                Err(e) => {
                    warn!(
                        "CursorStore: failed to deserialize cursor for '{}': {:#}",
                        name, e
                    );
                    continue;
                }
            };
            out.insert(name, point);
        }

        Ok(out)
    }

    async fn save(&self, entries: &HashMap<String, CursorEntry>) -> Result<(), CursorSaveError> {
        let mut failed = Vec::new();

        for (name, entry) in entries {
            let key = Self::key_for(name);

            let val = match bincode::serialize(entry) {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        "CursorStore: failed to serialize cursor for '{}': {:#}",
                        name, e
                    );
                    failed.push(name.clone());
                    continue;
                }
            };

            if let Err(e) = self.partition.insert(&key, val) {
                warn!(
                    "CursorStore: failed to write cursor for '{}': {:#}",
                    name, e
                );
                failed.push(name.clone());
                continue;
            }
        }

        if failed.is_empty() {
            Ok(())
        } else {
            Err(CursorSaveError { failed })
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to save cursor tips for: {failed:?}")]
pub struct CursorSaveError {
    pub failed: Vec<String>,
}
