use crate::{BlockHash, Slot};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ChainSyncCommand {
    ChangeSyncPoint { slot: Slot, hash: BlockHash },
}
