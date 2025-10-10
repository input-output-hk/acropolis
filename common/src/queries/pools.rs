use crate::{
    queries::governance::VoteRecord, rational_number::RationalNumber, BlockHash, KeyHash,
    PoolEpochState, PoolMetadata, PoolRegistration, PoolRetirement, PoolUpdateEvent, Relay,
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
    GetPoolsTotalBlocksMinted {
        pools_operators: Vec<KeyHash>,
    },
    GetPoolInfo {
        pool_id: KeyHash,
    },
    GetPoolHistory {
        pool_id: KeyHash,
    },
    GetPoolMetadata {
        pool_id: KeyHash,
    },
    GetPoolRelays {
        pool_id: KeyHash,
    },
    GetPoolDelegators {
        pool_id: KeyHash,
    },
    GetPoolTotalBlocksMinted {
        pool_id: KeyHash,
    },
    GetPoolBlockHashes {
        pool_id: KeyHash,
    },
    GetPoolBlockHashesByEpoch {
        pool_id: KeyHash,
        epoch: u64,
    },
    GetPoolUpdates {
        pool_id: KeyHash,
    },
    GetPoolVotes {
        pool_id: KeyHash,
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
    PoolsTotalBlocksMinted(Vec<u64>),
    PoolInfo(PoolRegistration),
    PoolHistory(Vec<PoolEpochState>),
    PoolMetadata(PoolMetadata),
    PoolRelays(Vec<Relay>),
    PoolDelegators(PoolDelegators),
    PoolTotalBlocksMinted(u64),
    PoolBlockHashes(Vec<BlockHash>),
    PoolBlockHashesByEpoch(Vec<BlockHash>),
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
