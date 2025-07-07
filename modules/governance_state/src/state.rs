//! Acropolis Governance State: State storage

use acropolis_common::{
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, SPOStakeDistributionMessage,
        GovernanceOutcomesMessage,
        GovernanceProceduresMessage, Message, ProtocolParamsMessage,
    },
    BlockInfo, ConwayParams, DRepCredential, DataHash, EnactStateElem, Era, GovActionId,
    GovernanceAction, GovernanceOutcome, GovernanceOutcomeVariant, KeyHash, Lovelace,
    ProposalProcedure, SingleVoterVotes,
    TreasuryWithdrawalsAction, Voter, VotesCount, VotingOutcome, VotingProcedure,
};
use crate::VotingRegistrationState;
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
    spo_stake: HashMap<KeyHash, u64>,

    proposals: HashMap<GovActionId, (u64, ProposalProcedure)>,
    votes: HashMap<GovActionId, HashMap<Voter, (DataHash, VotingProcedure)>>,
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

            proposals: HashMap::new(),
            votes: HashMap::new(),

            drep_stake: HashMap::new(),
            spo_stake: HashMap::new(),
        }
    }

    pub fn advance_era(&mut self, new_era: &Era) {
        self.current_era = new_era.clone();
    }

    pub async fn handle_protocol_parameters(
        &mut self,
        message: &ProtocolParamsMessage,
    ) -> Result<()> {
        if message.params.conway.is_some() {
            self.conway = message.params.conway.clone();
        }

        Ok(())
    }

    pub async fn handle_drep_stake(
        &mut self,
        drep_message: &DRepStakeDistributionMessage,
        spo_message: &SPOStakeDistributionMessage
    ) -> Result<()> {
        self.drep_stake_messages_count += 1;
        self.drep_stake = HashMap::from_iter(drep_message.dreps.iter().cloned());
        self.spo_stake = HashMap::from_iter(spo_message.spos.iter().cloned());

        Ok(())
    }

    /// Implementation of new governance message processing handle
    pub async fn handle_governance(
        &mut self,
        block: &BlockInfo,
        governance_message: &GovernanceProceduresMessage,
    ) -> Result<()> {
        if block.era < Era::Conway {
            if !(governance_message.proposal_procedures.is_empty() &&
                governance_message.voting_procedures.is_empty())
            {
                bail!("Non-empty governance message for pre-conway block {block:?}");
            }
            return Ok(())
        }

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
        info!("Inserting proposal procedure: {:?}", proc);
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
        transaction: &DataHash,
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

    fn proportional_count_drep_comm(
        &self,
        drep: &RationalNumber,
        comm: &RationalNumber,
    ) -> Result<(u64, u64)> {
        let d = (drep * self.voting_state.registered_dreps).ceil().to_integer();
        let c = (comm * self.voting_state.committee_size).ceil().to_integer();
        Ok((d, c))
    }

    fn proportional_count(
        &self,
        pool: &RationalNumber,
        drep: &RationalNumber,
        comm: &RationalNumber,
    ) -> Result<VotesCount> {
        let mut votes = VotesCount::zero();
        votes.pool = (pool * self.voting_state.registered_spos).ceil().to_integer();
        (votes.drep, votes.committee) = self.proportional_count_drep_comm(drep, comm)?;
        Ok(votes)
    }

    fn full_count(
        &self,
        pool: &RationalNumber,
        drep: &RationalNumber,
        comm: &RationalNumber,
    ) -> Result<VotesCount> {
        let mut votes = VotesCount::zero();
        votes.pool = (pool * self.voting_state.total_spos).ceil().to_integer();
        (votes.drep, votes.committee) = self.proportional_count_drep_comm(drep, comm)?;
        Ok(votes)
    }

    /// Returns protocol parameter types, needed to determine voting thresholds for
    /// the parameter(s) updates.
    fn get_protocol_param_types(&self, p: &ProtocolParamUpdate) -> ProtocolParamType {
        let mut result = ProtocolParamType::none();

        if p.max_block_body_size.is_some()
            || p.max_block_header_size.is_some()
            || p.max_transaction_size.is_some()
            || p.max_value_size.is_some()
            || p.max_block_ex_units.is_some()
            || p.governance_action_deposit.is_some()
            || p.ada_per_utxo_byte.is_some()
            || p.minfee_refscript_cost_per_byte.is_some()
            || p.minfee_a.is_some()
            || p.minfee_b.is_some()
        {
            result |= ProtocolParamType::SecurityProperty;
        }

        if p.max_block_body_size.is_some()
            || p.max_transaction_size.is_some()
            || p.max_block_header_size.is_some()
            || p.max_value_size.is_some()
            || p.max_tx_ex_units.is_some()
            || p.max_block_ex_units.is_some()
            || p.max_collateral_inputs.is_some()
        {
            result |= ProtocolParamType::NetworkGroup;
        }

        if p.minfee_a.is_some()
            || p.minfee_b.is_some()
            || p.key_deposit.is_some()
            || p.pool_deposit.is_some()
            || p.expansion_rate.is_some()
            || p.treasury_growth_rate.is_some()
            || p.min_pool_cost.is_some()
            || p.ada_per_utxo_byte.is_some()
            || p.execution_costs.is_some()
            || p.minfee_refscript_cost_per_byte.is_some()
        {
            result |= ProtocolParamType::EconomicGroup;
        }

        if p.pool_pledge_influence.is_some()
            || p.maximum_epoch.is_some()
            || p.desired_number_of_stake_pools.is_some()
            || p.execution_costs.is_some()
            || p.collateral_percentage.is_some()
        {
            result |= ProtocolParamType::TechnicalGroup;
        }

        if p.pool_voting_thresholds.is_some()
            || p.drep_voting_thresholds.is_some()
            || p.governance_action_validity_period.is_some()
            || p.governance_action_deposit.is_some()
            || p.drep_deposit.is_some()
            || p.drep_inactivity_period.is_some()
            || p.min_committee_size.is_some()
            || p.committee_term_limit.is_some()
        {
            result |= ProtocolParamType::GovernanceGroup;
        }

        result
    }

    /// Computes necessary votes count to accept proposal `pp`, according to
    /// actual parameters. The result is triple of votes' thresholds (as fraction of the
    /// total corresponding votes count): (Pool, DRep, Committee)
    fn get_action_thresholds(
        &self,
        pp: &ProposalProcedure,
        thresholds: &ConwayParams,
    ) -> Result<VotesCount> {
        let d = &thresholds.d_rep_voting_thresholds;
        let p = &thresholds.pool_voting_thresholds;
        let c = &thresholds.committee;
        let zero = &RationalNumber::ZERO;
        let one = &RationalNumber::ONE;

        match &pp.gov_action {
            GovernanceAction::ParameterChange(action) => {
                let param_types = self.get_protocol_param_types(&action.protocol_param_update);

                let mut p_th = zero;
                let mut d_th = zero;

                if param_types.contains(ProtocolParamType::SecurityProperty) {
                    p_th = &p.security_voting_threshold;
                }
                if param_types.contains(ProtocolParamType::EconomicGroup) {
                    d_th = max(d_th, &d.pp_economic_group);
                }
                if param_types.contains(ProtocolParamType::NetworkGroup) {
                    d_th = max(d_th, &d.pp_network_group);
                }
                if param_types.contains(ProtocolParamType::TechnicalGroup) {
                    d_th = max(d_th, &d.pp_technical_group);
                }
                if param_types.contains(ProtocolParamType::GovernanceGroup) {
                    d_th = max(d_th, &d.pp_governance_group);
                }

                self.proportional_count(p_th, d_th, &c.threshold)
            }
            GovernanceAction::HardForkInitiation(_) => self.full_count(
                &p.hard_fork_initiation,
                &d.hard_fork_initiation,
                &c.threshold,
            ),
            GovernanceAction::TreasuryWithdrawals(_) => {
                self.proportional_count(zero, &d.treasury_withdrawal, &c.threshold)
            }
            GovernanceAction::NoConfidence(_) => self.proportional_count(
                &p.motion_no_confidence.clone(),
                &d.motion_no_confidence.clone(),
                zero,
            ),
            GovernanceAction::UpdateCommittee(_) => {
                if thresholds.committee.is_empty() {
                    self.proportional_count(
                        &p.committee_no_confidence,
                        &d.committee_no_confidence,
                        zero,
                    )
                } else {
                    self.proportional_count(&p.committee_normal, &d.committee_normal, zero)
                }
            }
            GovernanceAction::NewConstitution(_) => {
                self.proportional_count(zero, &d.update_constitution, &c.threshold)
            }
            GovernanceAction::Information => self.proportional_count(one, one, zero),
        }
    }

    /// Checks whether action_id can be considered finally accepted
    fn is_finally_accepted(&self, voting_state: &VotingRegistrationState, action_id: &GovActionId) -> Result<VotingOutcome> {
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
                        self.spo_stake.get(pool).inspect(|v| votes.pool += *v);
                    }
                }
            }
        }
        votes
    }

    /// Checks whether action is expired at the beginning of new_epoch
    fn is_expired(&self, new_epoch: u64, action_id: &GovActionId) -> Result<bool> {
        info!(
            "Checking whether {} is expired at new epoch {}",
            action_id, new_epoch
        );
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

    fn recalculate_voting_state(&self) -> Result<VotingRegistrationState> {
        let drep_stake = self.drep_stake.iter().map(|(_dr,lov)| lov).sum();

        let committee_usize = self.get_conway_params()?.committee.members.len();
        let committee = committee_usize.try_into().or_else(
            |e| Err(anyhow!("Commitee size: conversion usize -> u64 failed, {e}"))
        )?;

        let spo_stake = self.spo_stake.iter().map(|(_sp,lov)| lov).sum();

        Ok(VotingRegistrationState::new(spo_stake, spo_stake, drep_stake, committee))
    }

    /// Loops through all actions and checks their status for the new_epoch
    /// All incoming data (parameters for the epoch, drep distribution, etc)
    /// should already be actual at this moment.
    pub fn process_new_epoch(&mut self, new_block: &BlockInfo) 
        -> Result<GovernanceOutcomesMessage> 
    {
        let mut output = GovernanceOutcomesMessage::default();
        if self.current_era < Era::Conway {
            // Processes new epoch acts on old events.
            // However, there should be no governance events before
            // Conway era start.
            return Ok(output);
        }

        let voting_state = self.recalculate_voting_state()?;

        let actions = self.proposals.keys().map(|a| a.clone()).collect::<Vec<_>>();
        let mut wdr = 0;
        let mut ens = 0;
        let mut rej = 0;

        for action_id in actions.iter() {
            info!("Epoch {}: processing action {}", new_block.epoch, action_id);
            match self.process_one_proposal(new_block.epoch, &voting_state, &action_id) {
                Err(e) => error!("Error processing governance {action_id}: {e}"),
                Ok(None) => (),
                Ok(Some(out)) if out.accepted => {
                    let mut action_to_perform = GovernanceOutcomeVariant::NoAction;

                    if let Some(elem) = Self::pack_as_enact_state_elem(&out.procedure) {
                        action_to_perform = GovernanceOutcomeVariant::EnactStateElem(elem);
                        ens += 1;
                    } else if let Some(wt) = Self::retrieve_withdrawal(&out.procedure) {
                        action_to_perform = GovernanceOutcomeVariant::TreasuryWithdrawal(wt);
                        wdr += 1;
                    }

                    output.outcomes.push(GovernanceOutcome {
                        voting: out,
                        action_to_perform,
                    })
                }
                Ok(Some(out)) => {
                    rej += 1;
                    output.outcomes.push(GovernanceOutcome {
                        voting: out,
                        action_to_perform: GovernanceOutcomeVariant::NoAction,
                    })
                }
            }
        }

        info!(
            "Epoch {} ({}): {}, total {} actions, {ens} enacts, {wdr} withdrawals, {rej} rejected",
            voting_state, new_block.epoch, new_block.era, output.outcomes.len()
        );
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
    pub fn list_proposals(&self) -> Result<Vec<(GovActionId, ProposalProcedure)>> {
        let mut result = Vec::new();
        for (action, (_epoch, prop)) in self.proposals.iter() {
            result.push((action.clone(), prop.clone()))
        }
        Ok(result)
    }

    /// Get list of casted votes
    pub fn list_votes(&self) -> Result<Vec<(GovActionId, Voter, DataHash, VotingProcedure)>> {
        let mut result = Vec::new();
        for (action, all_votes) in self.votes.iter() {
            for (voter, (transaction, voting_proc)) in all_votes.iter() {
                result.push((
                    action.clone(),
                    voter.clone(),
                    transaction.clone(),
                    voting_proc.clone(),
                ));
            }
        }
        Ok(result)
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }
}
