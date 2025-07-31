use std::collections::HashMap;

use crate::{
    Anchor, DRepCredential, GovActionId, Lovelace, ProposalProcedure, Voter, VotingProcedure,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum GovernanceStateQuery {
    GetDRepsList,
    GetDRepInfo { drep_credential: DRepCredential },
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
pub enum GovernanceStateQueryResponse {
    DRepsList(DRepsList),
    DRepInfo(DRepInfo),
    DRepDelegators(DRepDelegatorAddresses),
    DRepMetadata(DRepMetadata),
    DRepUpdates(DRepUpdates),
    DRepVotes(DRepVotes),
    ProposalsList(ProposalsList),
    ProposalInfo(ProposalInfo),
    ProposalParameters(ProposalParameters),
    ProposalWithdrawals(ProposalWithdrawals),
    ProposalVotes(ProposalVotes),
    ProposalMetadata(ProposalMetadata),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepsList {
    pub dreps: Vec<DRepCredential>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepInfo {
    pub deposit: Lovelace,
    pub anchor: Option<Anchor>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepDelegatorAddresses {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepMetadata {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepUpdates {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepVotes {}

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
    pub votes: HashMap<Voter, (Vec<u8>, VotingProcedure)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalMetadata {}
