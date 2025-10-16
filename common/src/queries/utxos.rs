use crate::{UTxOIdentifier, Value};

pub const DEFAULT_UTXOS_QUERY_TOPIC: (&str, &str) =
    ("utxo-state-query-topic", "cardano.query.utxos");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UTxOStateQuery {
    GetUTxOsSum {
        utxo_identifiers: Vec<UTxOIdentifier>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UTxOStateQueryResponse {
    UTxOsSum(Value),
    NotFound,
    Error(String),
}
