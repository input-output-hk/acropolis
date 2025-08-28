use crate::ProtocolParams;

pub const DEFAULT_PARAMETERS_QUERY_TOPIC: (&str, &str) =
    ("parameters-state-query-topic", "cardano.query.parameters");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ParametersStateQuery {
    GetLatestParameters,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ParametersStateQueryResponse {
    LatestParameters(LatestParameters),

    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatestParameters {
    pub parameters: ProtocolParams,
}
