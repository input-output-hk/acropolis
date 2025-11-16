use crate::queries::errors::QueryError;
use crate::{messages::EpochActivityMessage, protocol_params::ProtocolParams, PoolId};

pub const DEFAULT_EPOCHS_QUERY_TOPIC: (&str, &str) =
    ("epochs-state-query-topic", "cardano.query.epochs");

pub const DEFAULT_HISTORICAL_EPOCHS_QUERY_TOPIC: (&str, &str) = (
    "historical-epochs-state-query-topic",
    "cardano.query.historical.epochs",
);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EpochsStateQuery {
    GetLatestEpoch,

    // Served from historical epochs state
    GetEpochInfo { epoch_number: u64 },
    GetNextEpochs { epoch_number: u64 },
    GetPreviousEpochs { epoch_number: u64 },
    GetEpochStakeDistribution { epoch_number: u64 },
    GetEpochStakeDistributionByPool { epoch_number: u64 },
    GetLatestEpochBlocksMintedByPool { spo_id: PoolId },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EpochsStateQueryResponse {
    LatestEpoch(LatestEpoch),
    EpochInfo(EpochInfo),
    NextEpochs(NextEpochs),
    PreviousEpochs(PreviousEpochs),
    EpochStakeDistribution(EpochStakeDistribution),
    EpochStakeDistributionByPool(EpochStakeDistributionByPool),
    LatestEpochBlocksMintedByPool(u64),
    Error(QueryError),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatestEpoch {
    pub epoch: EpochActivityMessage,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatestEpochParameters {
    pub parameters: ProtocolParams,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochInfo {
    pub epoch: EpochActivityMessage,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NextEpochs {
    pub epochs: Vec<EpochActivityMessage>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviousEpochs {
    pub epochs: Vec<EpochActivityMessage>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochStakeDistribution {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochStakeDistributionByPool {}
