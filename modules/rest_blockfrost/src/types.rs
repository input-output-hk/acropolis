use acropolis_common::VotingProcedure;
use serde::Serialize;

/// REST response structure for proposal votes
#[derive(Serialize)]
pub struct VoteRest {
    pub transaction: String,
    pub voting_procedure: VotingProcedure,
}
