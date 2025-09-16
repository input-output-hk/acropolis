//! Acropolis Governance State: State storage

use crate::alonzo_babbage_voting::AlonzoBabbageVoting;
use crate::VotingRegistrationState;
use acropolis_common::{
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, GovernanceOutcomesMessage,
        GovernanceProceduresMessage, Message, ProtocolParamsMessage, SPOStakeDistributionMessage,
    },
    protocol_params::ConwayParams,
    BlockInfo, DRepCredential, DelegatedStake, EnactStateElem, Era, GovActionId, GovernanceAction,
    GovernanceOutcome, GovernanceOutcomeVariant, KeyHash, Lovelace, ProposalProcedure,
    SingleVoterVotes, TreasuryWithdrawalsAction, TxHash, Voter, VotesCount, VotingOutcome,
    VotingProcedure,
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::Context;
use hex::ToHex;
use std::{collections::HashMap, sync::Arc};
use tracing::{debug, error, info};

pub struct State {
    pub enact_state_topic: String,
    pub context: Arc<Context<Message>>,

    proposal_count: usize,

    pub action_proposal_count: usize,
    pub votes_count: usize,
    pub drep_stake_messages_count: usize,

    current_era: Era,
    conway: Option<ConwayParams>,
    drep_stake: HashMap<DRepCredential, Lovelace>,
    spo_stake: HashMap<KeyHash, DelegatedStake>,

    alonzo_babbage_voting: AlonzoBabbageVoting,
    proposals: HashMap<GovActionId, (u64, ProposalProcedure)>,
    votes: HashMap<GovActionId, HashMap<Voter, (TxHash, VotingProcedure)>>,
}

impl State {
    pub fn new(context: Arc<Context<Message>>, enact_state_topic: String) -> Self {
        Self {
            context,
            enact_state_topic,

            proposal_count: 0,
            action_proposal_count: 0,
            votes_count: 0,
            drep_stake_messages_count: 0,

            conway: None,
            current_era: Era::default(),

            alonzo_babbage_voting: AlonzoBabbageVoting::default(),
            proposals: HashMap::new(),
            votes: HashMap::new(),

            drep_stake: HashMap::new(),
            spo_stake: HashMap::new(),
        }
    }

    /// Update current fields to new epoch values. The function should be called
    /// after all block processing is done.
    pub fn advance_epoch(&mut self, epoch_blk: &BlockInfo) -> Result<()> {
        if !epoch_blk.new_epoch {
            bail!("Block {epoch_blk:?} must start a new epoch");
        }
        self.current_era = epoch_blk.era.clone(); // If era is the same -- no problem
        self.alonzo_babbage_voting.advance_epoch(epoch_blk);
        Ok(())
    }

    pub async fn handle_protocol_parameters(
        &mut self,
        message: &ProtocolParamsMessage,
    ) -> Result<()> {
        if let Some(p) = &message.params.shelley {
            self.alonzo_babbage_voting.update_parameters(p.epoch_length, p.update_quorum);
        }
        if message.params.conway.is_some() {
            self.conway = message.params.conway.clone();
        }

        Ok(())
    }

    pub async fn handle_drep_stake(
        &mut self,
        drep_message: &DRepStakeDistributionMessage,
        spo_message: &SPOStakeDistributionMessage,
    ) -> Result<()> {
        self.drep_stake_messages_count += 1;
        self.drep_stake = HashMap::from_iter(drep_message.dreps.iter().cloned());
        self.spo_stake = HashMap::from_iter(spo_message.spos.iter().cloned());

        Ok(())
    }

    /// Implementation of governance message processing handle
    pub async fn handle_governance(
        &mut self,
        block: &BlockInfo,
        governance_message: &GovernanceProceduresMessage,
    ) -> Result<()> {
        if block.era < Era::Conway {
            // Alonzo-Babbage governance
            if !(governance_message.proposal_procedures.is_empty()
                && governance_message.voting_procedures.is_empty())
            {
                bail!("Unexpected Conway governance procedures in pre-Conway block {block:?}");
            }

            if !governance_message.alonzo_babbage_updates.is_empty() {
                if let Err(e) = self
                    .alonzo_babbage_voting
                    .process_update_proposals(block, &governance_message.alonzo_babbage_updates)
                {
                    error!("Error handling Babbage governance_message: '{e}'");
                }
            }
        } else {
            // Conway governance
            for pproc in &governance_message.proposal_procedures {
                self.proposal_count += 1;
                if let Err(e) = self.insert_proposal_procedure(block.epoch, pproc) {
                    error!("Error handling governance_message: '{}'", e);
                }
            }

            for (trans, vproc) in &governance_message.voting_procedures {
                for (voter, voter_votes) in vproc.votes.iter() {
                    if let Err(e) = self.insert_voting_procedure(voter, trans, voter_votes) {
                        error!(
                            "Error handling governance voting block {}, trans {}: '{}'",
                            block.number,
                            trans.encode_hex::<String>(),
                            e
                        );
                    }
                    self.votes_count += voter_votes.voting_procedures.len();
                }
            }
        }

        Ok(())
    }

    pub fn get_conway_params(&self) -> Result<&ConwayParams> {
        self.conway.as_ref().ok_or_else(|| anyhow!("Conway parameters not available"))
    }

    #[allow(dead_code)]
    fn have_committee(&self) -> bool {
        !self.conway.iter().any(|c| c.committee.is_empty())
    }

    /// Update proposals memory cache
    fn insert_proposal_procedure(&mut self, epoch: u64, proc: &ProposalProcedure) -> Result<()> {
        self.action_proposal_count += 1;
        let prev = self.proposals.insert(proc.gov_action_id.clone(), (epoch, proc.clone()));
        if let Some(prev) = prev {
            return Err(anyhow!(
                "Governance procedure {} already exists! New: {:?}, old: {:?}",
                proc.gov_action_id,
                (epoch, proc),
                prev
            ));
        }
        Ok(())
    }

    /// Update votes memory cache
    fn insert_voting_procedure(
        &mut self,
        voter: &Voter,
        transaction: &TxHash,
        voter_votes: &SingleVoterVotes,
    ) -> Result<()> {
        for (action_id, procedure) in voter_votes.voting_procedures.iter() {
            let votes = self.votes.entry(action_id.clone()).or_insert_with(|| HashMap::new());
            if let Some((prev_trans, prev_vote)) =
                votes.insert(voter.clone(), (transaction.clone(), procedure.clone()))
            {
                // Re-voting is allowed; new vote must be treated as the proper one,
                // older is to be discarded.
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("Governance vote by {} for {} already registered! New: {:?}, old: {:?} from {}",
                        voter, action_id, procedure, prev_vote, prev_trans.encode_hex::<String>()
                    );
                }
            }
        }
        Ok(())
    }

    /// Checks whether action_id can be considered finally accepted
    fn is_finally_accepted(
        &self,
        voting_state: &VotingRegistrationState,
        action_id: &GovActionId,
    ) -> Result<VotingOutcome> {
        let (_epoch, proposal) = self
            .proposals
            .get(action_id)
            .ok_or_else(|| anyhow!("action {} not found", action_id))?;
        let conway_params = self.get_conway_params()?;
        let threshold = voting_state.get_action_thresholds(proposal, conway_params)?;

        let votes = self.get_actual_votes(action_id);
        let accepted = votes.majorizes(&threshold);
        info!("Proposal {action_id}: votes {votes}, thresholds {threshold}, result {accepted}");

        Ok(VotingOutcome {
            procedure: proposal.clone(),
            votes_cast: votes,
            votes_threshold: threshold,
            accepted,
        })
    }

    /// Should be called when voting is over
    fn end_voting(&mut self, action_id: &GovActionId) -> Result<()> {
        self.votes.remove(&action_id);
        self.proposals.remove(&action_id);

        Ok(())
    }

    /// Returns actual votes: (Pool votes, DRep votes, committee votes)
    fn get_actual_votes(&self, action_id: &GovActionId) -> VotesCount {
        let mut votes = VotesCount::zero();
        if let Some(all_votes) = self.votes.get(&action_id) {
            for voter in all_votes.keys() {
                match voter {
                    Voter::ConstitutionalCommitteeKey(_) => votes.committee += 1,
                    Voter::ConstitutionalCommitteeScript(_) => votes.committee += 1,
                    Voter::DRepKey(key) => {
                        self.drep_stake
                            .get(&DRepCredential::AddrKeyHash(key.clone()))
                            .inspect(|v| votes.drep += *v);
                    }
                    Voter::DRepScript(script) => {
                        self.drep_stake
                            .get(&DRepCredential::ScriptHash(script.clone()))
                            .inspect(|v| votes.drep += *v);
                    }
                    Voter::StakePoolKey(pool) => {
                        self.spo_stake.get(pool).inspect(|ds| votes.pool += ds.live);
                    }
                }
            }
        }
        votes
    }

    /// Checks whether `action_id` is expired at the beginning of `new_epoch`.
    fn is_expired(&self, new_epoch: u64, action_id: &GovActionId) -> Result<bool> {
        info!("Checking whether {action_id} is expired at new epoch {new_epoch}");
        let (proposal_epoch, _proposal) = self
            .proposals
            .get(action_id)
            .ok_or_else(|| anyhow!("action {} not found", action_id))?;

        Ok(proposal_epoch + self.get_conway_params()?.gov_action_lifetime as u64 <= new_epoch)
    }

    fn pack_as_enact_state_elem(p: &ProposalProcedure) -> Option<EnactStateElem> {
        match &p.gov_action {
            GovernanceAction::HardForkInitiation(_hf) => None,
            GovernanceAction::TreasuryWithdrawals(_wt) => None,
            GovernanceAction::Information => None,

            GovernanceAction::ParameterChange(pc) => {
                Some(EnactStateElem::Params(pc.protocol_param_update.clone()))
            }
            GovernanceAction::NewConstitution(nc) => {
                Some(EnactStateElem::Constitution(nc.new_constitution.clone()))
            }
            GovernanceAction::UpdateCommittee(uc) => {
                Some(EnactStateElem::Committee(uc.data.clone()))
            }
            GovernanceAction::NoConfidence(_) => Some(EnactStateElem::NoConfidence),
        }
    }

    fn retrieve_withdrawal(p: &ProposalProcedure) -> Option<TreasuryWithdrawalsAction> {
        if let GovernanceAction::TreasuryWithdrawals(ref action) = p.gov_action {
            Some(action.clone())
        } else {
            None
        }
    }

    /// Checks and updates action_id state at the start of new_epoch
    /// If the action is accepted, returns accepted ProposalProcedure.
    fn process_one_proposal(
        &mut self,
        new_epoch: u64,
        voting_state: &VotingRegistrationState,
        action_id: &GovActionId,
    ) -> Result<Option<VotingOutcome>> {
        let outcome = self.is_finally_accepted(voting_state, &action_id)?;
        let expired = self.is_expired(new_epoch, &action_id)?;
        if outcome.accepted || expired {
            self.end_voting(&action_id)?;
            info!(
                "New epoch {new_epoch}: voting for {action_id} outcome: {}, expired: {expired}",
                outcome.accepted
            );
            return Ok(Some(outcome));
        }

        Ok(None)
    }

    fn finalize_conway_voting(
        &mut self,
        new_block: &BlockInfo,
        voting_state: &VotingRegistrationState,
    ) -> Result<Vec<GovernanceOutcome>> {
        let mut outcome = Vec::<GovernanceOutcome>::new();
        let actions = self.proposals.keys().map(|a| a.clone()).collect::<Vec<_>>();

        for action_id in actions.iter() {
            info!("Epoch {}: processing action {}", new_block.epoch, action_id);
            match self.process_one_proposal(new_block.epoch, &voting_state, &action_id) {
                Err(e) => error!("Error processing governance {action_id}: {e}"),
                Ok(None) => (),
                Ok(Some(out)) if out.accepted => {
                    let mut action_to_perform = GovernanceOutcomeVariant::NoAction;

                    if let Some(elem) = Self::pack_as_enact_state_elem(&out.procedure) {
                        action_to_perform = GovernanceOutcomeVariant::EnactStateElem(elem);
                    } else if let Some(wt) = Self::retrieve_withdrawal(&out.procedure) {
                        action_to_perform = GovernanceOutcomeVariant::TreasuryWithdrawal(wt);
                    }

                    outcome.push(GovernanceOutcome {
                        voting: out,
                        action_to_perform,
                    })
                }
                Ok(Some(out)) => outcome.push(GovernanceOutcome {
                    voting: out,
                    action_to_perform: GovernanceOutcomeVariant::NoAction,
                }),
            }
        }

        Ok(outcome)
    }

    fn recalculate_voting_state(&self) -> Result<VotingRegistrationState> {
        let drep_stake = self.drep_stake.iter().map(|(_dr, lov)| lov).sum();

        let committee_usize = self.get_conway_params()?.committee.members.len();
        let committee = committee_usize.try_into().or_else(|e| {
            Err(anyhow!(
                "Commitee size: conversion usize -> u64 failed, {e}"
            ))
        })?;

        let spo_stake = self.spo_stake.iter().map(|(_sp, ds)| ds.live).sum();

        Ok(VotingRegistrationState::new(
            spo_stake, spo_stake, drep_stake, committee,
        ))
    }

    /// Loops through all actions and checks their status for the new_epoch
    /// All incoming data (parameters for the epoch, drep distribution, etc)
    /// should already be actual at this moment.
    pub fn process_new_epoch(
        &mut self,
        new_block: &BlockInfo,
    ) -> Result<GovernanceOutcomesMessage> {
        let mut output = GovernanceOutcomesMessage::default();
        output.alonzo_babbage_outcomes = self.alonzo_babbage_voting.finalize_voting(new_block)?;

        if self.current_era >= Era::Conway {
            let voting_state = self.recalculate_voting_state()?;
            let outcome = self.finalize_conway_voting(&new_block, &voting_state)?;
            let acc = outcome.iter().filter(|oc| oc.voting.accepted).count();

            info!(
                "Conway voting, epoch {} ({}): {voting_state}, total {} actions, {acc} accepted",
                new_block.epoch,
                new_block.era,
                outcome.len()
            );
            output.conway_outcomes = outcome;
        }
        return Ok(output);
    }

    async fn log_stats(&self) {
        info!("props: {}, props_with_id: {}, votes: {}, stored proposal procedures: {}, drep stake msgs (size): {} ({})",
            self.proposal_count, self.action_proposal_count, self.votes_count, self.proposals.len(),
            self.drep_stake_messages_count, self.drep_stake.len(),
        );

        for (action_id, procedure) in self.votes.iter() {
            info!(
                "{}{} => {}",
                action_id,
                [" (absent)", ""][self.proposals.contains_key(action_id) as usize],
                procedure.len()
            )
        }
    }

    pub async fn send(&self, block: &BlockInfo, message: GovernanceOutcomesMessage) -> Result<()> {
        let packed_message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::GovernanceOutcomes(message),
        )));
        let context = self.context.clone();
        let enact_state_topic = self.enact_state_topic.clone();

        tokio::spawn(async move {
            context
                .message_bus
                .publish(&enact_state_topic, packed_message)
                .await
                .unwrap_or_else(|e| tracing::error!("Failed to publish: {e}"));
        });
        Ok(())
    }

    /// Get list of actual voting proposals
    pub fn list_proposals(&self) -> Vec<GovActionId> {
        self.proposals.keys().cloned().collect()
    }

    /// Get details for a specific proposal
    pub fn get_proposal(&self, id: &GovActionId) -> Option<ProposalProcedure> {
        self.proposals.get(id).map(|(_epoch, prop)| prop.clone())
    }

    /// Get list of votes for a specific proposal
    pub fn get_proposal_votes(
        &self,
        proposal_id: &GovActionId,
    ) -> Result<HashMap<Voter, (TxHash, VotingProcedure)>> {
        match self.votes.get(proposal_id) {
            Some(all_votes) => Ok(all_votes.clone()),
            None => Err(anyhow::anyhow!(
                "Governance action: {:?} not found",
                proposal_id
            )),
        }
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }
}
