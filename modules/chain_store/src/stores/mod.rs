use acropolis_common::TxHash;
use anyhow::{Context, Result};

mod fjall;

pub trait Store {
    fn insert_block(&self, block: &Block) -> Result<()>;

    fn get_block_by_hash(&self, hash: &[u8]) -> Result<Block>;
    fn get_block_by_slot(&self, slot: u64) -> Result<Block>;
    fn get_block_by_number(&self, number: u64) -> Result<Block>;
    fn get_latest_block(&self) -> Result<Block>;
}

#[derive(Debug, PartialEq, Eq)]
pub struct Block {
    slot: u64,
    number: u64,
    hash: Vec<u8>,
    txs: Vec<Vec<u8>>,
}

pub(crate) fn hash_tx(tx: &[u8]) -> Result<TxHash> {
    let tx = pallas_traverse::MultiEraTx::decode(tx).context("could not decode tx")?;
    Ok(TxHash::from(*tx.hash()))
}
