use crate::queries::errors::QueryError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NetworkStateQuery {
    GetNetworkInformation,
    GetEraSummary,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NetworkStateQueryResponse {
    NetworkInformation(NetworkInformation),
    EraSummary(EraSummary),
    Error(QueryError),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkInformation {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EraSummary {}
