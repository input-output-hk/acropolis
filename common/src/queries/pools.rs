#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PoolsStateQuery {
    GetPoolsList,
    GetPoolsListExtended,
    GetPoolsRetiredList,
    GetPoolsRetiringList,
    GetPoolInfo { pool_id: Vec<u8> },
    GetPoolHistory { pool_id: Vec<u8> },
    GetPoolMetadata { pool_id: Vec<u8> },
    GetPoolRelays { pool_id: Vec<u8> },
    GetPoolDelegators { pool_id: Vec<u8> },
    GetPoolBlocks { pool_id: Vec<u8> },
    GetPoolUpdates { pool_id: Vec<u8> },
    GetPoolVotes { pool_id: Vec<u8> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PoolsStateQueryResponse {
    PoolsList(PoolsList),
    PoolsListExtended(PoolsListExtended),
    PoolsRetiredList(PoolsRetiredList),
    PoolsRetiringList(PoolsRetiringList),
    PoolInfo(PoolInfo),
    PoolHistory(PoolHistory),
    PoolMetadata(PoolMetadata),
    PoolRelays(PoolRelays),
    PoolDelegators(PoolDelegators),
    PoolBlocks(PoolBlocks),
    PoolUpdates(PoolUpdates),
    PoolVotes(PoolVotes),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsList {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsListExtended {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsRetiredList {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsRetiringList {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolInfo {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolHistory {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolMetadata {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolRelays {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolDelegators {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolBlocks {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolUpdates {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolVotes {}
