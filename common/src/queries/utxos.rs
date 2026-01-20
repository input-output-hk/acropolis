use crate::queries::errors::QueryError;
use crate::{BlockInfo, UTXOValue, UTxOIdentifier, Value};

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
    /// Cancel all unredeemed AVVM (Byron redeem) UTxOs at the Allegra hard fork boundary.
    /// Returns the count and total lovelace value of cancelled UTxOs.
    /// This is a one-time operation that happens at epoch 236 on mainnet.
    /// The block_info is used to generate AddressDeltas for the cancelled UTxOs.
    CancelRedeemUtxos { block_info: BlockInfo },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UTxOStateQueryResponse {
    UTxOsSum(Value),
    UTxOs(Vec<UTXOValue>),
    /// Response to CancelRedeemUtxos: (count of cancelled UTxOs, total lovelace value)
    RedeemUtxosCancelled { count: usize, total_value: u64 },
    Error(QueryError),
}
