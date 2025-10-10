use acropolis_common::{BlockInfo, TxHash};
use anyhow::{Context, Result};

pub mod fjall;

pub trait Store: Send + Sync {
    fn insert_block(&self, info: &BlockInfo, block: &[u8]) -> Result<()>;

    fn get_block_by_hash(&self, hash: &[u8]) -> Result<Option<Block>>;
    fn get_block_by_slot(&self, slot: u64) -> Result<Option<Block>>;
    fn get_block_by_number(&self, number: u64) -> Result<Option<Block>>;
    fn get_blocks_by_number_range(&self, min_number: u64, max_number: u64) -> Result<Vec<Block>>;
    fn get_block_by_epoch_slot(&self, epoch: u64, epoch_slot: u64) -> Result<Option<Block>>;
    fn get_latest_block(&self) -> Result<Option<Block>>;
}

#[derive(Debug, PartialEq, Eq, minicbor::Decode, minicbor::Encode)]
pub struct Block {
    #[n(0)]
    pub bytes: Vec<u8>,
    #[n(1)]
    pub extra: ExtraBlockData,
}

#[derive(Debug, PartialEq, Eq, minicbor::Decode, minicbor::Encode)]
pub struct ExtraBlockData {
    #[n(0)]
    pub epoch: u64,
    #[n(1)]
    pub epoch_slot: u64,
    #[n(2)]
    pub timestamp: u64,
}

pub(crate) fn extract_tx_hashes(block: &[u8]) -> Result<Vec<TxHash>> {
    let block = pallas_traverse::MultiEraBlock::decode(block).context("could not decode block")?;
    Ok(block.txs().into_iter().map(|tx| TxHash(*tx.hash())).collect())
}
