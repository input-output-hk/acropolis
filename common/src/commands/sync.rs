use crate::{BlockHash, Slot};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SyncCommand {
    ChangeSyncPoint { slot: Slot, hash: BlockHash },
}
