use crate::{BlockHash, Slot};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ChainSyncCommand {
    FindIntersect { point: Point },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Point {
    Origin,
    Specific(Slot, BlockHash),
}

impl Point {
    pub fn slot_or_default(&self) -> u64 {
        match self {
            Point::Origin => 0,
            Point::Specific(slot, _) => *slot,
        }
    }
}
