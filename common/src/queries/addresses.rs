use crate::{Address, AddressTotalsEntry, TxHash, UTxOIdentifier};

pub const DEFAULT_ADDRESS_QUERY_TOPIC: (&str, &str) =
    ("address-state-query-topic", "cardano.query.address");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AddressStateQuery {
    GetAddressTotals { address_key: Address },
    GetAddressUTxOs { address_key: Address },
    GetAddressAssetUTxOs { address_key: Address, asset_id: u64 },
    GetAddressTransactions { address_key: Address },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AddressStateQueryResponse {
    AddressTotals(AddressTotalsEntry),
    AddressUTxOs(Vec<UTxOIdentifier>),
    AddressAssetUTxOs(Vec<UTxOIdentifier>),
    AddressTransactions(Vec<TxHash>),
    NotFound,
    Error(String),
}
