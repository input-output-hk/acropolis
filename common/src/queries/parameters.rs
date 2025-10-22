use crate::protocol_params::ProtocolParams;

pub const DEFAULT_PARAMETERS_QUERY_TOPIC: (&str, &str) =
    ("parameters-state-query-topic", "cardano.query.parameters");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ParametersStateQuery {
    GetLatestEpochParameters,
    GetEpochParameters { epoch_number: u64 },
    GetNetworkName,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ParametersStateQueryResponse {
    LatestEpochParameters(ProtocolParams),
    EpochParameters(ProtocolParams),
    NetworkName(String),

    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatestParameters {
    pub parameters: ProtocolParams,
}
