use crate::{Address, AddressTotalsEntry, TxIdentifier, UTxOIdentifier};

pub const DEFAULT_ADDRESS_QUERY_TOPIC: (&str, &str) =
    ("address-state-query-topic", "cardano.query.address");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AddressStateQuery {
    GetAddressTotals { address: Address },
    GetAddressUTxOs { address: Address },
    GetAddressTransactions { address: Address },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AddressStateQueryResponse {
    AddressTotals(AddressTotalsEntry),
    AddressUTxOs(Vec<UTxOIdentifier>),
    AddressTransactions(Vec<TxIdentifier>),
    NotFound,
    Error(String),
}
