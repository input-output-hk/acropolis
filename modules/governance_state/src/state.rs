//! Acropolis SPOState: State storage

use std::collections::HashMap;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use hex::ToHex;
use tracing::{debug, error, info};
use acropolis_common::messages::{GovernanceProceduresMessage};
use acropolis_common::{DataHash, GovActionId, ProposalProcedure, SerialisedMessageHandler, Voter, VotingProcedure};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub prev_sequence: u64,
    pub proposal_count: usize,
    pub action_proposal_count: usize,
    pub votes_count: usize,

    pub proposals: HashMap<GovActionId, ProposalProcedure>,
    pub votes: HashMap<GovActionId, HashMap<Voter, (DataHash, VotingProcedure)>>,
}

#[async_trait]
impl SerialisedMessageHandler<GovernanceProceduresMessage> for State {
    async fn handle(&mut self, msg: &GovernanceProceduresMessage) -> Result<()> {
        if let Err(e) = self.handle_impl(msg).await {
            error!("Error processing message {:?}: {}", msg, e)
        }

        Ok(())
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            prev_sequence: 0,
            proposals: HashMap::new(),
            votes: HashMap::new(),
            proposal_count: 0,
            action_proposal_count: 0,
            votes_count: 0,
        }
    }

    fn insert_proposal_procedure(&mut self, proc: &ProposalProcedure) -> Result<()> {
        self.action_proposal_count += 1;
        let prev = self.proposals.insert(proc.gov_action_id.clone(), proc.clone());
        if let Some(prev) = prev {
            return Err(anyhow!("Governance procedure {} already exists! New: {:?}, old: {:?}",
                proc.gov_action_id, proc, prev
            ));
        }
        Ok(())
    }

    fn insert_voting_procedure(&mut self, voter: &Voter, transaction: &DataHash, elementary_votes: &HashMap<GovActionId, VotingProcedure>) -> Result<()> {
        for (action_id, procedure) in elementary_votes.iter() {
            let votes = self.votes.entry(action_id.clone()).or_insert_with(|| HashMap::new());
            if let Some((prev_trans, prev_vote)) = votes.insert(voter.clone(), (transaction.clone(), procedure.clone())) {
                // Re-voting is allowed; new vote must be treated as the proper one, older is to be discarded.
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("Governance vote by {} for {} already registered! New: {:?}, old: {:?} from {}",
                        voter, action_id, procedure, prev_vote, prev_trans.encode_hex::<String>()
                    );
                }
            }
        }
        Ok(())
    }

    async fn log_stats(&self) {
        info!("props: {}, props_with_id: {}, votes: {}, stored proposal procedures: {}",
            self.proposal_count, self.action_proposal_count, self.votes_count, self.proposals.len()
        );

        for (action_id, procedure) in self.votes.iter() {
            info!("{}{} => {}",
                action_id,
                if self.proposals.contains_key(action_id) {""} else {" (absent)"},
                procedure.len()
            )
        }
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }

    pub async fn handle_impl(&mut self, governance_message: &GovernanceProceduresMessage) -> Result<()> {
        if self.prev_sequence >= governance_message.sequence {
            error!("Governance message sequence number {} going backwards: prev {}",
                governance_message.sequence, self.prev_sequence
            );
        }
        self.prev_sequence = governance_message.sequence;

        for pproc in &governance_message.proposal_procedures {
            self.proposal_count += 1;
            if let Err(e) = self.insert_proposal_procedure(pproc) {
                error!("Error handling governance_message {}: '{}'", governance_message.sequence, e);
            }
        }
        for (trans, vproc) in &governance_message.voting_procedures {
            for (voter, elementary_votes) in vproc.votes.iter() {
                if let Err(e) = self.insert_voting_procedure(voter, trans, elementary_votes) {
                    error!("Error handling governance voting block {}, trans {}: '{}'",
                        governance_message.block.number, trans.encode_hex::<String>(), e
                    );
                }
                self.votes_count += elementary_votes.len();
            }
        }
        Ok(())
    }
}
