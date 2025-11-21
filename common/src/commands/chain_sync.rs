use crate::{BlockHash, Slot};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ChainSyncCommand {
    FindIntersect { slot: Slot, hash: BlockHash },
}
