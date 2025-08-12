use crate::{messages::EpochActivityMessage, KeyHash, ProtocolParams};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EpochsStateQuery {
    GetLatestEpoch,
    GetLatestEpochParameters,
    GetEpochInfo { epoch_number: u64 },
    GetNextEpochs { epoch_number: u64 },
    GetPreviousEpochs { epoch_number: u64 },
    GetEpochStakeDistribution { epoch_number: u64 },
    GetEpochStakeDistributionByPool { epoch_number: u64 },
    GetEpochBlockDistribution { epoch_number: u64 },
    GetEpochBlockDistributionByPool { epoch_number: u64 },
    GetEpochParameters { epoch_number: u64 },

    // Pools related queries
    GetTotalBlocksMintedByPools { vrf_key_hashes: Vec<KeyHash> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EpochsStateQueryResponse {
    LatestEpoch(LatestEpoch),
    LatestEpochParameters(LatestEpochParameters),
    EpochInfo(EpochInfo),
    NextEpochs(NextEpochs),
    PreviousEpochs(PreviousEpochs),
    EpochStakeDistribution(EpochStakeDistribution),
    EpochStakeDistributionByPool(EpochStakeDistributionByPool),
    EpochBlockDistribution(EpochBlockDistribution),
    EpochBlockDistributionByPool(EpochBlockDistributionByPool),
    EpochParameters(EpochParameters),

    // Pools related responses
    TotalBlocksMintedByPools(TotalBlocksMintedByPools),

    NotFound,
    Error(String),
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
pub struct EpochInfo {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NextEpochs {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviousEpochs {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochStakeDistribution {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochStakeDistributionByPool {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochBlockDistribution {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochBlockDistributionByPool {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochParameters {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TotalBlocksMintedByPools {
    // this is in same order of vrf_key_hashes from EpochsStateQuery::GetTotalBlocksMintedByPools
    pub total_blocks_minted: Vec<usize>,
}
