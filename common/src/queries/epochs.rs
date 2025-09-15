use crate::{messages::EpochActivityMessage, protocol_params::ProtocolParams, BlockHash, KeyHash};

pub const DEFAULT_EPOCHS_QUERY_TOPIC: (&str, &str) =
    ("epochs-state-query-topic", "cardano.query.epochs");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EpochsStateQuery {
    GetLatestEpoch,
    GetEpochInfo { epoch_number: u64 },
    GetNextEpochs { epoch_number: u64 },
    GetPreviousEpochs { epoch_number: u64 },
    GetEpochStakeDistribution { epoch_number: u64 },
    GetEpochStakeDistributionByPool { epoch_number: u64 },
    GetEpochBlockDistribution { epoch_number: u64 },
    GetEpochBlockDistributionByPool { epoch_number: u64 },

    // Pools related queries
    GetBlocksMintedByPools { vrf_key_hashes: Vec<KeyHash> },
    GetTotalBlocksMintedByPools { vrf_key_hashes: Vec<KeyHash> },
    GetBlocksMintedInfoByPool { vrf_key_hash: KeyHash },
    GetBlockHashesByPool { vrf_key_hash: KeyHash },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EpochsStateQueryResponse {
    LatestEpoch(LatestEpoch),
    EpochInfo(EpochInfo),
    NextEpochs(NextEpochs),
    PreviousEpochs(PreviousEpochs),
    EpochStakeDistribution(EpochStakeDistribution),
    EpochStakeDistributionByPool(EpochStakeDistributionByPool),
    EpochBlockDistribution(EpochBlockDistribution),
    EpochBlockDistributionByPool(EpochBlockDistributionByPool),

    // Pools related responses
    BlocksMintedByPools(BlocksMintedByPools),
    TotalBlocksMintedByPools(TotalBlocksMintedByPools),
    BlocksMintedInfoByPool(BlocksMintedInfoByPool),
    BlockHashesByPool(BlockHashesByPool),

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
pub struct EpochInfo {
    pub epoch: EpochActivityMessage,
}

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
pub struct BlocksMintedByPools {
    // this is in same order of vrf_key_hashes from EpochsStateQuery::BlocksMintedByPools
    pub blocks_minted: Vec<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TotalBlocksMintedByPools {
    // this is in same order of vrf_key_hashes from EpochsStateQuery::TotalBlocksMinted
    pub total_blocks_minted: Vec<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlocksMintedInfoByPool {
    pub total_blocks_minted: u64,
    pub epoch_blocks_minted: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockHashesByPool {
    pub hashes: Vec<BlockHash>,
}
