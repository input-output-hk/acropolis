use std::collections::HashMap;

use crate::queries::errors::QueryError;
use crate::{
    Anchor, DRepCredential, GovActionId, Lovelace, ProposalProcedure, StakeAddress, TxHash,
    TxIdentifier, Vote, Voter, VotingProcedure,
};

pub const DEFAULT_DREPS_QUERY_TOPIC: (&str, &str) =
    ("drep-state-query-topic", "cardano.query.dreps");
pub const DEFAULT_GOVERNANCE_QUERY_TOPIC: (&str, &str) =
    ("governance-state-query-topic", "cardano.query.governance");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum GovernanceStateQuery {
    GetDRepsList,
    GetDRepInfoWithDelegators { drep_credential: DRepCredential },
    GetDRepDelegators { drep_credential: DRepCredential },
    GetDRepMetadata { drep_credential: DRepCredential },
    GetDRepUpdates { drep_credential: DRepCredential },
    GetDRepVotes { drep_credential: DRepCredential },
    GetProposalsList,
    GetProposalInfo { proposal: GovActionId },
    GetProposalParameters { proposal: GovActionId },
    GetProposalWithdrawals { proposal: GovActionId },
    GetProposalVotes { proposal: GovActionId },
    GetProposalMetadata { proposal: GovActionId },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum GovernanceStateQueryResponse {
    DRepsList(DRepsList),
    DRepInfoWithDelegators(DRepInfoWithDelegators),
    DRepDelegators(DRepDelegatorAddresses),
    DRepMetadata(Option<Option<Anchor>>),
    DRepUpdates(DRepUpdates),
    DRepVotes(DRepVotes),
    ProposalsList(ProposalsList),
    ProposalInfo(ProposalInfo),
    ProposalParameters(ProposalParameters),
    ProposalWithdrawals(ProposalWithdrawals),
    ProposalVotes(ProposalVotes),
    ProposalMetadata(ProposalMetadata),
    Error(QueryError),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepsList {
    pub dreps: Vec<DRepCredential>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepInfo {
    pub deposit: Lovelace,
    pub retired: bool,
    pub expired: bool,
    pub active_epoch: Option<u64>,
    pub last_active_epoch: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepInfoWithDelegators {
    pub info: DRepInfo,
    pub delegators: Vec<StakeAddress>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepDelegatorAddresses {
    pub addresses: Vec<StakeAddress>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepUpdates {
    pub updates: Vec<DRepUpdateEvent>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepUpdateEvent {
    pub tx_identifier: TxIdentifier,
    pub cert_index: u64,
    pub action: DRepActionUpdate,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum DRepActionUpdate {
    Registered,
    Updated,
    Deregistered,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct DRepVotes {
    pub votes: Vec<VoteRecord>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct VoteRecord {
    pub tx_hash: TxHash,
    pub vote_index: u32,
    pub vote: Vote,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalsList {
    pub proposals: Vec<GovActionId>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalInfo {
    pub procedure: ProposalProcedure,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalParameters {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalWithdrawals {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalVotes {
    pub votes: HashMap<Voter, (TxHash, VotingProcedure)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalMetadata {}
