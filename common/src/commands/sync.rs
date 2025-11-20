use crate::{BlockHash, Slot};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncCommand {
    slot: Slot,
    hash: BlockHash,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SyncCommandResponse {
    Success,
    Error(String),
}
