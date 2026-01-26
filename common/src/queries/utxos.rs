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
    GetAllUTxOsSumAtShelleyStart,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UTxOStateQueryResponse {
    UTxOsSum(Value),
    UTxOs(Vec<UTXOValue>),
    LovelaceSum(u64),
    Error(QueryError),
}
