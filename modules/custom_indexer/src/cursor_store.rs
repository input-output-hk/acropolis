use std::{
    collections::{HashMap, VecDeque},
    path::Path,
};

use acropolis_common::Point;
use anyhow::Result;
use caryatid_sdk::async_trait;
use fjall::{Database, Keyspace, KeyspaceCreateOptions};
use tokio::sync::Mutex;
use tracing::warn;

#[derive(
    Debug,
    Clone,
    Default,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Eq,
    minicbor::Decode,
    minicbor::Encode,
)]
pub struct CursorEntry {
    #[n(0)]
    pub points: VecDeque<Point>,
    #[n(1)]
    pub next_tx: Option<u64>,
}

#[async_trait]
pub trait CursorStore: Send + Sync + 'static {
    async fn load(&self) -> Result<HashMap<String, CursorEntry>>;
    async fn save(&self, entries: &HashMap<String, CursorEntry>) -> Result<()>;
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

#[async_trait]
impl CursorStore for InMemoryCursorStore {
    async fn load(&self) -> Result<HashMap<String, CursorEntry>> {
        let guard = self.entries.lock().await;
        Ok(guard.clone())
    }

    async fn save(&self, entries: &HashMap<String, CursorEntry>) -> Result<()> {
        let mut guard = self.entries.lock().await;
        *guard = entries.clone();
        Ok(())
    }
}

// Fjall backed cursor store (Retains last stored point)
const CURSOR_PREFIX: &str = "cursor/";

pub struct FjallCursorStore {
    keyspace: Keyspace,
}

impl FjallCursorStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let database = Database::builder(path).open()?;
        let keyspace = database.keyspace("cursor", KeyspaceCreateOptions::default)?;

        Ok(Self { keyspace })
    }

    fn key_for(name: &str) -> String {
        format!("{CURSOR_PREFIX}{name}")
    }

    fn name_from_key(key: &[u8]) -> Option<String> {
        let s = std::str::from_utf8(key).ok()?;
        s.strip_prefix(CURSOR_PREFIX).map(|n| n.to_string())
    }

    fn prefix_iter(&self) -> impl Iterator<Item = fjall::Guard> + '_ {
        self.keyspace.prefix(CURSOR_PREFIX)
    }
}

#[async_trait]
impl CursorStore for FjallCursorStore {
    async fn load(&self) -> Result<HashMap<String, CursorEntry>> {
        let mut out = HashMap::new();
        for next in self.prefix_iter() {
            let (key_bytes, val_bytes) = match next.into_inner() {
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

    async fn save(&self, entries: &HashMap<String, CursorEntry>) -> Result<()> {
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

            if let Err(e) = self.keyspace.insert(&key, val) {
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
            Err(anyhow::anyhow!("failed to save cursors"))
        }
    }
}
