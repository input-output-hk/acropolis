use crate::queries::errors::QueryError;

pub const DEFAULT_SPDD_QUERY_TOPIC: (&str, &str) = ("spdd-state-query-topic", "cardano.query.spdd");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SPDDStateQuery {
    GetEpochTotalActiveStakes { epoch: u64 },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SPDDStateQueryResponse {
    EpochTotalActiveStakes(u64),
    Error(QueryError),
}
