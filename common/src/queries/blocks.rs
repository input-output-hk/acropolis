use crate::KeyHash;

pub const DEFAULT_BLOCKS_QUERY_TOPIC: (&str, &str) =
    ("blocks-state-query-topic", "cardano.query.blocks");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlocksStateQuery {
    GetLatestBlock,
    GetLatestBlockTransactions,
    GetLatestBlockTransactionsCBOR,
    GetBlockInfo { block_key: Vec<u8> },
    GetNextBlocks { block_key: Vec<u8> },
    GetPreviousBlocks { block_key: Vec<u8> },
    GetBlockBySlot { slot: u64 },
    GetBlockByEpochSlot { epoch: u64, slot: u64 },
    GetBlockTransactions { block_key: Vec<u8> },
    GetBlockTransactionsCBOR { block_key: Vec<u8> },
    GetBlockInvolvedAddresses { block_key: Vec<u8> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlocksStateQueryResponse {
    LatestBlock(BlockInfo),
    LatestBlockTransactions(BlockTransactions),
    LatestBlockTransactionsCBOR(BlockTransactionsCBOR),
    BlockInfo(BlockInfo),
    NextBlocks(NextBlocks),
    PreviousBlocks(PreviousBlocks),
    BlockBySlot(BlockInfo),
    BlockByEpochSlot(BlockInfo),
    BlockTransactions(BlockTransactions),
    BlockTransactionsCBOR(BlockTransactionsCBOR),
    BlockInvolvedAddresses(BlockInvolvedAddresses),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockInfo {
    pub timestamp: u64,
    pub number: u64,
    pub hash: Vec<u8>,
    pub slot: u64,
    pub epoch: u64,
    pub epoch_slot: u64,
    pub issuer_vkey: Option<Vec<u8>>,
    pub size: u64,
    pub tx_count: u64,
    pub output: Option<u64>,
    pub fees: Option<u64>,
    pub block_vrf: Option<Vec<u8>>,
    pub op_cert: Option<KeyHash>,
    pub op_cert_counter: Option<u64>,
    pub previous_block: Option<Vec<u8>>,
    pub next_block: Option<Vec<u8>>,
    pub confirmations: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NextBlocks {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviousBlocks {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockTransactions {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockTransactionsCBOR {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockInvolvedAddresses {}
