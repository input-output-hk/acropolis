use crate::{
    KeyHash, PoolEpochState, PoolMetadata, PoolRegistration, PoolRetirement, Relay, StakeCredential,
};

pub const DEFAULT_POOLS_QUERY_TOPIC: (&str, &str) =
    ("pools-state-query-topic", "cardano.query.pools");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PoolsStateQuery {
    GetPoolsList,
    GetPoolsListWithInfo,
    GetPoolsRetiredList,
    GetPoolsRetiringList,
    GetPoolsActiveStakes {
        pools_operators: Vec<Vec<u8>>,
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
    GetAccountsBalances {
        stake_keys: Vec<KeyHash>,
    },
    GetPoolBlocks {
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
    AccountsBalances(AccountsBalances),
    PoolBlocks(PoolBlocks),
    PoolUpdates(PoolUpdates),
    PoolVotes(PoolVotes),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsList {
    pub pool_operators: Vec<KeyHash>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsListWithInfo {
    pub pools: Vec<(KeyHash, PoolRegistration)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsRetiredList {
    pub retired_pools: Vec<PoolRetirement>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolsRetiringList {
    // pool id, retiring epoch
    pub retiring_pools: Vec<PoolRetirement>,
}

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
pub struct PoolHistory {
    pub history: Vec<PoolEpochState>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolRelays {
    pub relays: Vec<Relay>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolDelegators {
    pub delegators: Vec<StakeCredential>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountsBalances {
    pub balances: Vec<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolBlocks {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolUpdates {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolVotes {}
