#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AddressStateQuery {
    GetAddressInfo { address_key: Vec<u8> },
    GetAddressInfoExtended { address_key: Vec<u8> },
    GetAddressAssetTotals { address_key: Vec<u8> },
    GetAddressUTxOs { address_key: Vec<u8> },
    GetAddressAssetUTxOs { address_key: Vec<u8> },
    GetAddressTransactions { address_key: Vec<u8> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AddressStateQueryResponse {
    AddressInfo(AddressInfo),
    AddressInfoExtended(AddressInfoExtended),
    AddressAssetTotals(AddressAssetTotals),
    AddressUTxOs(AddressUTxOs),
    AddressAssetUTxOs(AddressAssetUTxOs),
    AddressTransactions(AddressTransactions),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressInfo {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressInfoExtended {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressAssetTotals {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressUTxOs {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressAssetUTxOs {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressTransactions {}
