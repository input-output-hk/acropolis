#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlocksStateQuery {
    GetLatestBlock,
    GetLatestBlockTransactions,
    GetLatestBlockTransactionsCBOR,
    GetBlockInfo { block_key: Vec<u8> },
    GetNextBlocks { block_key: Vec<u8> },
    GetPreviousBlocks { block_key: Vec<u8> },
    GetBlockBySlot { slot_key: Vec<u8> },
    GetBlockByEpochSlot { slot_key: Vec<u8> },
    GetBlockTransactions { block_key: Vec<u8> },
    GetBlockTransactionsCBOR { block_key: Vec<u8> },
    GetBlockInvolvedAddresses { block_key: Vec<u8> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlocksStateQueryResponse {
    LatestBlock(LatestBlock),
    LatestBlockTransactions(LatestBlockTransactions),
    LatestBlockTransactionsCBOR(LatestBlockTransactionsCBOR),
    BlockInfo(BlockInfo),
    NextBlocks(NextBlocks),
    PreviousBlocks(PreviousBlocks),
    BlockBySlot(BlockBySlot),
    BlockByEpochSlot(BlockByEpochSlot),
    BlockTransactions(BlockTransactions),
    BlockTransactionsCBOR(BlockTransactionsCBOR),
    BlockInvolvedAddresses(BlockInvolvedAddresses),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatestBlock {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatestBlockTransactions {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatestBlockTransactionsCBOR {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockInfo {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NextBlocks {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviousBlocks {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockBySlot {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockByEpochSlot {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockTransactions {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockTransactionsCBOR {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockInvolvedAddresses {}
