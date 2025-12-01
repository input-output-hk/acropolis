use std::future::Future;

use acropolis_common::Point;
use anyhow::Result;

pub trait CursorStore: Send + Sync + 'static {
    fn load(&self) -> impl Future<Output = Result<Option<Point>>> + Send;
    fn save(&mut self, point: &Point) -> impl Future<Output = Result<()>> + Send;
}

pub struct InMemoryCursorStore {
    cursor: Option<Point>,
}
impl InMemoryCursorStore {
    pub fn new(cursor: Point) -> Self {
        Self {
            cursor: Some(cursor),
        }
    }
}
impl CursorStore for InMemoryCursorStore {
    async fn load(&self) -> Result<Option<Point>> {
        Ok(self.cursor.clone())
    }

    async fn save(&mut self, cursor: &Point) -> Result<()> {
        self.cursor = Some(cursor.clone());
        Ok(())
    }
}
