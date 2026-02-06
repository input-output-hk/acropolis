use crate::queries::errors::QueryError;
use crate::{ShelleyAddressPointer, UTXOValue, UTxOIdentifier, Value};
use std::collections::HashMap;

pub const DEFAULT_UTXOS_QUERY_TOPIC: (&str, &str) =
    ("utxo-state-query-topic", "cardano.query.utxos");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UTxOStateQuery {
    GetUTxOsSum {
        utxo_identifiers: Vec<UTxOIdentifier>,
    },
    GetUTxOs {
        utxo_identifiers: Vec<UTxOIdentifier>,
    },
    GetAllUTxOsSumAtShelleyStart,
    GetAvvmCancelledValue,
    /// Get total lovelace held in pointer address UTxOs, grouped by pointer.
    /// Used at Conway hard fork to remove pointer address stake from the distribution
    /// (per Conway spec 9.1.2: pointer addresses no longer count towards stake).
    GetPointerAddressValues,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UTxOStateQueryResponse {
    UTxOsSum(Value),
    UTxOs(Vec<UTXOValue>),
    LovelaceSum(u64),
    AvvmCancelledValue(Option<u64>),
    /// Map of pointer -> total lovelace for all unspent pointer address UTxOs
    PointerAddressValues(HashMap<ShelleyAddressPointer, u64>),
    Error(QueryError),
}
