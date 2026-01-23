use crate::queries::errors::QueryError;
use crate::{UTXOValue, UTxOIdentifier, Value};

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
    /// Get the total lovelace value of AVVM UTxOs cancelled at Allegra boundary.
    /// Returns None if cancellation hasn't happened yet.
    GetAvvmCancelledValue,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UTxOStateQueryResponse {
    UTxOsSum(Value),
    UTxOs(Vec<UTXOValue>),
    /// Response to GetAvvmCancelledValue: None if not yet cancelled, Some(value) after cancellation
    AvvmCancelledValue(Option<Value>),
    Error(QueryError),
}
