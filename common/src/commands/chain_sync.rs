use crate::Point;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ChainSyncCommand {
    FindIntersect(Point),
}
