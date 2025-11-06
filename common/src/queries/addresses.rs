use crate::{Address, AddressTotals, ShelleyAddress, TxIdentifier, UTxOIdentifier};

pub const DEFAULT_ADDRESS_QUERY_TOPIC: (&str, &str) =
    ("address-state-query-topic", "cardano.query.address");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AddressStateQuery {
    GetAddressTotals { address: Address },
    GetAddressUTxOs { address: Address },
    GetAddressTransactions { address: Address },

    // Accounts related queries
    GetAddressesTotals { addresses: Vec<ShelleyAddress> },
    GetAddressesUTxOs { addresses: Vec<ShelleyAddress> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AddressStateQueryResponse {
    AddressTotals(AddressTotals),
    AddressUTxOs(Vec<UTxOIdentifier>),
    AddressTransactions(Vec<TxIdentifier>),

    // Accounts related queries
    AddressesTotals(AddressTotals),
    AddressesUTxOs(Vec<UTxOIdentifier>),
    NotFound,
    Error(String),
}
