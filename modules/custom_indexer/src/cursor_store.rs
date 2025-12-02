use std::{future::Future, path::Path, sync::Mutex};

use acropolis_common::Point;
use anyhow::Result;
use fjall::{Config, Keyspace, Partition, PartitionCreateOptions};

pub trait CursorStore: Send + Sync + 'static {
    fn load(&self) -> impl Future<Output = Result<Option<Point>>> + Send;
    fn save(&self, point: &Point) -> impl Future<Output = Result<()>> + Send;
}

// In memory cursor store (Not persisted across runs)
pub struct InMemoryCursorStore {
    cursor: Mutex<Option<Point>>,
}
impl InMemoryCursorStore {
    pub fn new(point: Point) -> Self {
        Self {
            cursor: Mutex::new(Some(point)),
        }
    }
}
impl CursorStore for InMemoryCursorStore {
    async fn load(&self) -> Result<Option<Point>> {
        let guard = self.cursor.lock().map_err(|_| anyhow::anyhow!("cursor mutex poisoned"))?;
        Ok(guard.as_ref().cloned())
    }

    async fn save(&self, point: &Point) -> Result<()> {
        let mut guard = self.cursor.lock().map_err(|_| anyhow::anyhow!("cursor mutex poisoned"))?;
        *guard = Some(point.clone());
        Ok(())
    }
}

// Fjall backed cursor store (Retains last stored point)
pub struct FjallCursorStore {
    cursor: Partition,
}

impl FjallCursorStore {
    pub fn new(path: impl AsRef<Path>, point: Point) -> Result<Self> {
        let cfg = Config::new(path);
        let keyspace = Keyspace::open(cfg)?;
        let partition = keyspace.open_partition("cursor", PartitionCreateOptions::default())?;

        // Use stored point if exists or initialize with provided point
        match partition.get("cursor")? {
            Some(_) => Ok(Self { cursor: partition }),
            None => {
                let raw = bincode::serialize(&point)?;
                partition.insert("cursor", raw)?;
                Ok(Self { cursor: partition })
            }
        }
    }
}

impl CursorStore for FjallCursorStore {
    async fn load(&self) -> Result<Option<Point>> {
        let raw = self.cursor.get("cursor")?;

        let Some(bytes) = raw else {
            return Ok(None);
        };

        let point: Point = bincode::deserialize(&bytes)?;

        Ok(Some(point))
    }

    async fn save(&self, point: &Point) -> Result<()> {
        let raw = bincode::serialize(point)?;

        self.cursor.insert("cursor", raw)?;

        Ok(())
    }
}
