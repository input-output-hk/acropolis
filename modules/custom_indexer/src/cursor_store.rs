use std::{collections::HashMap, future::Future, path::Path};

use acropolis_common::Point;
use anyhow::Result;
use fjall::{Config, Keyspace, Partition, PartitionCreateOptions};
use tokio::sync::Mutex;
use tracing::warn;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CursorEntry {
    pub tip: Point,
    pub halted: bool,
}

pub trait CursorStore: Send + Sync + 'static {
    fn load(&self) -> impl Future<Output = Result<HashMap<String, CursorEntry>>> + Send;
    fn save(
        &self,
        cursors: &HashMap<String, CursorEntry>,
    ) -> impl Future<Output = Result<(), CursorSaveError>> + Send;
}

// In memory cursor store (Not persisted across runs)
pub struct InMemoryCursorStore {
    cursors: Mutex<HashMap<String, CursorEntry>>,
}
impl InMemoryCursorStore {
    pub fn new() -> Self {
        Self {
            cursors: Mutex::new(HashMap::new()),
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
        let guard = self.cursors.lock().await;
        Ok(guard.clone())
    }

    async fn save(&self, cursors: &HashMap<String, CursorEntry>) -> Result<(), CursorSaveError> {
        let mut guard = self.cursors.lock().await;
        *guard = cursors.clone();
        Ok(())
    }
}

// Fjall backed cursor store (Retains last stored point)
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
}

impl CursorStore for FjallCursorStore {
    async fn load(&self) -> Result<HashMap<String, CursorEntry>> {
        let mut out = HashMap::new();
        let iter = self.partition.prefix("cursor/");
        for next in iter {
            let (key_bytes, val_bytes) = match next {
                Ok(r) => r,
                Err(e) => {
                    warn!("CursorStore: failed to read row: {:#}", e);
                    continue;
                }
            };

            let key = match String::from_utf8(key_bytes.to_vec()) {
                Ok(k) => k,
                Err(e) => {
                    warn!("CursorStore: invalid UTF-8 in key: {:#}", e);
                    continue;
                }
            };

            if let Some(name) = key.strip_prefix("cursor/") {
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
                out.insert(name.to_string(), point);
            }
        }

        Ok(out)
    }

    async fn save(&self, tips: &HashMap<String, CursorEntry>) -> Result<(), CursorSaveError> {
        let mut failed = Vec::new();

        for (name, point) in tips {
            let key = format!("cursor/{name}");

            let val = match bincode::serialize(point) {
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

#[derive(Debug)]
pub struct CursorSaveError {
    pub failed: Vec<String>,
}

impl std::fmt::Display for CursorSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to save cursor tips for: {:?}", self.failed)
    }
}

impl std::error::Error for CursorSaveError {}
