use crate::{queries::errors::QueryError, Lovelace, PoolId};

pub const DEFAULT_SPDD_QUERY_TOPIC: (&str, &str) = ("spdd-state-query-topic", "cardano.query.spdd");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SPDDStateQuery {
    GetEpochTotalActiveStakes { epoch: u64 },
    GetEpochSPDD { epoch: u64 },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SPDDStateQueryResponse {
    EpochTotalActiveStakes(u64),
    EpochSPDD(Vec<(PoolId, Lovelace)>),
    Error(QueryError),
}
