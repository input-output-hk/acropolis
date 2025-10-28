use crate::{queries::governance::VoteRecord, rational_number::RationalNumber, KeyHash, PoolEpochState, PoolId, PoolMetadata, PoolRegistration, PoolRetirement, PoolUpdateEvent, Relay};

pub const DEFAULT_POOLS_QUERY_TOPIC: (&str, &str) =
    ("pools-state-query-topic", "cardano.query.pools");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PoolsStateQuery {
    GetPoolsList,
    GetPoolsListWithInfo,
    GetPoolsRetiredList,
    GetPoolsRetiringList,
    GetPoolActiveStakeInfo {
        pool_operator: PoolId,
        epoch: u64,
    },
    GetPoolsActiveStakes {
        pools_operators: Vec<PoolId>,
        epoch: u64,
    },
    GetPoolsTotalBlocksMinted {
        pools_operators: Vec<PoolId>,
    },
    GetPoolInfo {
        pool_id: PoolId,
    },
    GetPoolHistory {
        pool_id: PoolId,
    },
    GetPoolMetadata {
        pool_id: PoolId,
    },
    GetPoolRelays {
        pool_id: PoolId,
    },
    GetPoolDelegators {
        pool_id: PoolId,
    },
    GetPoolTotalBlocksMinted {
        pool_id: PoolId,
    },
    GetBlocksByPool {
        pool_id: PoolId,
    },
    GetBlocksByPoolAndEpoch {
        pool_id: PoolId,
        epoch: u64,
    },
    GetPoolUpdates {
        pool_id: PoolId,
    },
    GetPoolVotes {
        pool_id: PoolId,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PoolsStateQueryResponse {
    PoolsList(Vec<PoolId>),
    PoolsListWithInfo(PoolsListWithInfo),
    PoolsRetiredList(Vec<PoolRetirement>),
    PoolsRetiringList(Vec<PoolRetirement>),
    PoolActiveStakeInfo(PoolActiveStakeInfo),
    PoolsActiveStakes(Vec<u64>),
    PoolsTotalBlocksMinted(Vec<u64>),
    PoolInfo(PoolRegistration),
    PoolHistory(Vec<PoolEpochState>),
    PoolMetadata(PoolMetadata),
    PoolRelays(Vec<Relay>),
    PoolDelegators(PoolDelegators),
    PoolTotalBlocksMinted(u64),
    // Vector of Block Heights
    BlocksByPool(Vec<u64>),
    // Vector of Block Heights
    BlocksByPoolAndEpoch(Vec<u64>),
    PoolUpdates(Vec<PoolUpdateEvent>),
    PoolVotes(Vec<VoteRecord>),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsListWithInfo {
    pub pools: Vec<(PoolId, PoolRegistration)>,
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
