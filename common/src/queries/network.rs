#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NetworkStateQuery {
    GetNetworkInformation,
    GetEraSummary,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NetworkStateQueryResponse {
    NetworkInformation(NetworkInformation),
    EraSummary(EraSummary),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkInformation {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EraSummary {}
