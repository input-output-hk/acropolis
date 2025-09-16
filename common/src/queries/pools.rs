use crate::{
    queries::governance::VoteRecord, rational_number::RationalNumber, KeyHash, PoolEpochState,
    PoolMetadata, PoolRegistration, PoolRetirement, PoolUpdateEvent, Relay,
};

pub const DEFAULT_POOLS_QUERY_TOPIC: (&str, &str) =
    ("pools-state-query-topic", "cardano.query.pools");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PoolsStateQuery {
    GetPoolsList,
    GetPoolsListWithInfo,
    GetPoolsRetiredList,
    GetPoolsRetiringList,
    GetPoolActiveStakeInfo {
        pool_operator: KeyHash,
        epoch: u64,
    },
    GetPoolsActiveStakes {
        pools_operators: Vec<KeyHash>,
        epoch: u64,
    },
    GetPoolInfo {
        pool_id: Vec<u8>,
    },
    GetPoolHistory {
        pool_id: Vec<u8>,
    },
    GetPoolMetadata {
        pool_id: Vec<u8>,
    },
    GetPoolRelays {
        pool_id: Vec<u8>,
    },
    GetPoolDelegators {
        pool_id: Vec<u8>,
    },
    GetPoolUpdates {
        pool_id: Vec<u8>,
    },
    GetPoolVotes {
        pool_id: Vec<u8>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PoolsStateQueryResponse {
    PoolsList(Vec<KeyHash>),
    PoolsListWithInfo(PoolsListWithInfo),
    PoolsRetiredList(Vec<PoolRetirement>),
    PoolsRetiringList(Vec<PoolRetirement>),
    PoolActiveStakeInfo(PoolActiveStakeInfo),
    PoolsActiveStakes(Vec<u64>),
    PoolInfo(PoolRegistration),
    PoolHistory(Vec<PoolEpochState>),
    PoolMetadata(PoolMetadata),
    PoolRelays(Vec<Relay>),
    PoolDelegators(PoolDelegators),
    PoolUpdates(Vec<PoolUpdateEvent>),
    PoolVotes(Vec<VoteRecord>),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsListWithInfo {
    pub pools: Vec<(KeyHash, PoolRegistration)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolActiveStakeInfo {
    pub active_stake: u64,
    pub active_size: RationalNumber,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolDelegators {
    pub delegators: Vec<(KeyHash, u64)>,
}
