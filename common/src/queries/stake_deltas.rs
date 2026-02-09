use crate::queries::errors::QueryError;
use crate::{ShelleyAddressPointer, StakeAddress};
use std::collections::HashMap;

pub const DEFAULT_STAKE_DELTAS_QUERY_TOPIC: (&str, &str) =
    ("stake-deltas-query-topic", "cardano.query.stake_deltas");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StakeDeltaQuery {
    /// Given a list of pointers, resolve each to its stake address (if known).
    /// Returns only pointers that could be resolved; unknown pointers are omitted.
    ResolvePointers {
        pointers: Vec<ShelleyAddressPointer>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StakeDeltaQueryResponse {
    /// Map of pointer -> resolved stake address (only for resolved pointers)
    ResolvedPointers(HashMap<ShelleyAddressPointer, StakeAddress>),
    Error(QueryError),
}
