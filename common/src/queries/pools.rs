use crate::PoolRegistration;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PoolsStateQuery {
    GetPoolsList,
    GetPoolsListWithInfo,
    GetPoolsRetiredList,
    GetPoolsRetiringList,
    GetPoolsActiveStakes { pools_operators: Vec<Vec<u8>>, epoch: u64 },
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
    PoolsListWithInfo(PoolsListWithInfo),
    PoolsRetiredList(PoolsRetiredList),
    PoolsRetiringList(PoolsRetiringList),
    PoolsActiveStakes(PoolsActiveStakes),
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
pub struct PoolsList {
    pub pool_operators: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsListWithInfo {
    pub pools: Vec<(Vec<u8>, PoolRegistration)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsRetiredList {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsRetiringList {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsActiveStakes {
    // this is in same order of pools_operator from PoolsStateQuery::GetPoolsActiveStakes
    pub active_stakes: Vec<u64>,
    // this is total active stake for current epoch
    pub total_active_stake: u64,
}

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
