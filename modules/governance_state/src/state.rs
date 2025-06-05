//! Acropolis Governance State: State storage

use std::{cmp::max, collections::{HashMap, VecDeque}};
use anyhow::{anyhow, Result};
use hex::ToHex;
use tracing::{debug, error, info};
use acropolis_common::{
    messages::{GovernanceProceduresMessage, DRepStakeDistributionMessage, GenesisCompleteMessage},
    rational_number::RationalNumber, BlockInfo,
    ConwayGenesisParams, DRepCredential, DataHash, GovActionId, GovernanceAction,
    KeyHash, Lovelace, ProposalProcedure, ProtocolParamType, ProtocolParamUpdate,
    SingleVoterVotes, Voter, VotingProcedure
};

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct VotingRegistrationState {
    total_spos: u64,
    registered_spos: u64,
    registered_dpres: u64,
    committee_size: u64
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct State {
    proposal_count: usize,

    pub action_proposal_count: usize,
    pub votes_count: usize,
    pub drep_stake_messages_count: usize,

    pub conway: Option<ConwayGenesisParams>,

    // epoch and procedure
    pub proposals: HashMap<GovActionId, (u64, ProposalProcedure)>,

    pub votes: HashMap<GovActionId, HashMap<Voter, (DataHash, VotingProcedure)>>,
    pub voting_state: VotingRegistrationState,

    drep_stake: HashMap<DRepCredential, Lovelace>,
    pub spo_stake: HashMap<KeyHash, u64>,

    // governance actions arrived before blockchain parameters are known
    pending_governance_actions: VecDeque<(BlockInfo, GovernanceProceduresMessage)>
}

impl State {
    pub fn new() -> Self {
        Self {
            proposal_count: 0,
            action_proposal_count: 0,
            votes_count: 0,
            drep_stake_messages_count: 0,

            conway: None,

            proposals: HashMap::new(),
            votes: HashMap::new(),
            voting_state: VotingRegistrationState::default(),

            drep_stake: HashMap::new(),
            spo_stake: HashMap::new(),
            pending_governance_actions: VecDeque::new()
        }
    }

    pub async fn handle_genesis(&mut self, message: &GenesisCompleteMessage) -> Result<()> {
        info!("Received genesis complete message; conway present = {}", 
            message.conway_genesis.is_some()
        );
        self.conway = message.conway_genesis.clone();
        self.replay_queue().await
    }

    pub async fn handle_governance(&mut self, block: &BlockInfo,
                                   new_msg: &GovernanceProceduresMessage) -> Result<()> {
        self.pending_governance_actions.push_back((block.clone(), new_msg.clone()));
        if !self.is_initalized() {
            tracing::warn!("Postponing message {:?}, module is not initalized yet", new_msg);
            Ok(())
        }
        else {
            self.replay_queue().await
        }
    }

    async fn replay_queue(&mut self) -> Result<()> {
        while let Some((block_info, pending_msg)) = self.pending_governance_actions.pop_front() {
            if let Err(e) = self.handle_impl(&block_info, &pending_msg).await {
                error!("Error processing message {:?}: {}", pending_msg, e)
            }
        }
        Ok(())
    }

    pub async fn handle_drep_stake(&mut self, message: &DRepStakeDistributionMessage) -> Result<()> {
        info!("Received drep stake distribution message: {} dreps", message.data.len());
        self.drep_stake_messages_count += 1;
        self.drep_stake = HashMap::from_iter(message.data.iter().cloned());
        Ok(())
    }

    pub fn get_conway_params(&self) -> Result<&ConwayGenesisParams> {
        self.conway.as_ref().ok_or_else(|| anyhow!("Conway parameters not available"))
    }

    fn is_initalized(&self) -> bool {
        self.conway.is_some()
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
            return Err(anyhow!("Governance procedure {} already exists! New: {:?}, old: {:?}",
                proc.gov_action_id, (epoch, proc), prev
            ));
        }
        Ok(())
    }

    /// Update votes memory cache
    fn insert_voting_procedure(
        &mut self, voter: &Voter, transaction: &DataHash, voter_votes: &SingleVoterVotes
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

    fn proportional_count_drep_comm(&self, drep: &RationalNumber, comm: &RationalNumber) -> Result<(u64, u64)> {
        let d = drep.proportion_of(self.voting_state.registered_dpres)?.round_up();
        let c = comm.proportion_of(self.voting_state.committee_size)?.round_up();
        Ok((d, c))
    }

    fn proportional_count(&self, pool: &RationalNumber, drep: &RationalNumber, comm: &RationalNumber) 
        -> Result<(u64, u64, u64)> 
    {
        let p = pool.proportion_of(self.voting_state.registered_spos)?.round_up();
        let (d,c) = self.proportional_count_drep_comm(drep, comm)?;
        Ok((p, d, c))
    }

    fn full_count(&self, pool: &RationalNumber, drep: &RationalNumber, comm: &RationalNumber) 
        -> Result<(u64, u64, u64)> 
    {
        let p = pool.proportion_of(self.voting_state.total_spos)?.round_up();
        let (d,c) = self.proportional_count_drep_comm(drep, comm)?;
        Ok((p, d, c))
    }

    #[allow(dead_code)]
    fn upd<T: Clone>(dst: &mut T, u: &Option<T>) {
        if let Some(u) = u { *dst = (*u).clone(); }
    }

    fn _update_conway_params(c: &mut ConwayGenesisParams, p: &ProtocolParamUpdate) {
        Self::upd(&mut c.pool_voting_thresholds, &p.pool_voting_thresholds);
        Self::upd(&mut c.d_rep_voting_thresholds, &p.drep_voting_thresholds);
        Self::upd(&mut c.committee_min_size, &p.min_committee_size);
        Self::upd(&mut c.committee_max_term_length, &p.committee_term_limit.map(|x| x as u32));
        Self::upd(&mut c.d_rep_activity, &p.drep_inactivity_period.map(|x| x as u32));
        Self::upd(&mut c.d_rep_deposit, &p.drep_deposit);
        Self::upd(&mut c.gov_action_deposit, &p.governance_action_deposit);
        Self::upd(&mut c.gov_action_lifetime, &p.governance_action_validity_period.map(|x| x as u32));
        Self::upd(&mut c.min_fee_ref_script_cost_per_byte, &p.minfee_refscript_cost_per_byte)
    }

    /// Returns protocol parameter types, needed to determine voting thresholds for
    /// the parameter(s) updates.
    fn get_protocol_param_types(&self, p: &ProtocolParamUpdate) -> ProtocolParamType {
        let mut result = ProtocolParamType::none();

        if p.max_block_body_size.is_some() ||
            p.max_block_header_size.is_some() ||
            p.max_transaction_size.is_some() ||
            p.max_value_size.is_some() ||
            p.max_block_ex_units.is_some() ||
            p.governance_action_deposit.is_some() ||
            p.ada_per_utxo_byte.is_some() ||
            p.minfee_refscript_cost_per_byte.is_some() ||
            p.minfee_a.is_some() ||
            p.minfee_b.is_some()
        {
            result |= ProtocolParamType::SecurityProperty;
        }

        if p.max_block_body_size.is_some() ||
            p.max_transaction_size.is_some() ||
            p.max_block_header_size.is_some() ||
            p.max_value_size.is_some() ||
            p.max_tx_ex_units.is_some() ||
            p.max_block_ex_units.is_some() ||
            p.max_collateral_inputs.is_some()
        {
            result |= ProtocolParamType::NetworkGroup;
        }

        if p.minfee_a.is_some() ||
            p.minfee_b.is_some() ||
            p.key_deposit.is_some() ||
            p.pool_deposit.is_some() ||
            p.expansion_rate.is_some() ||
            p.treasury_growth_rate.is_some() ||
            p.min_pool_cost.is_some() ||
            p.ada_per_utxo_byte.is_some() ||
            p.execution_costs.is_some() ||
            p.minfee_refscript_cost_per_byte.is_some()
        {
            result |= ProtocolParamType::EconomicGroup;
        }

        if p.pool_pledge_influence.is_some() ||
            p.maximum_epoch.is_some() ||
            p.desired_number_of_stake_pools.is_some() ||
            p.execution_costs.is_some() ||
            p.collateral_percentage.is_some()
        {
            result |= ProtocolParamType::TechnicalGroup;
        }

        if p.pool_voting_thresholds.is_some() ||
            p.drep_voting_thresholds.is_some() ||
            p.governance_action_validity_period.is_some() ||
            p.governance_action_deposit.is_some() ||
            p.drep_deposit.is_some() ||
            p.drep_inactivity_period.is_some() ||
            p.min_committee_size.is_some() ||
            p.committee_term_limit.is_some()
        {
            result |= ProtocolParamType::GovernanceGroup;
        }

        result
    }

    /// Computes necessary votes count to accept proposal `pp`, according to
    /// actual parameters. The result is triple of votes' thresholds (as fraction of the
    /// total corresponding votes count): (Pool, DRep, Committee)
    fn get_action_thresholds(&self, pp: &ProposalProcedure, thresholds: &ConwayGenesisParams) -> Result<(u64, u64, u64)> {
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

                if param_types.contains(ProtocolParamType::SecurityProperty) { p_th = &p.security_voting_threshold; }
                if param_types.contains(ProtocolParamType::EconomicGroup) { d_th = max(d_th, &d.pp_economic_group); }
                if param_types.contains(ProtocolParamType::NetworkGroup) { d_th = max(d_th, &d.pp_network_group); }
                if param_types.contains(ProtocolParamType::TechnicalGroup) { d_th = max(d_th, &d.pp_technical_group); }
                if param_types.contains(ProtocolParamType::GovernanceGroup) { d_th = max(d_th, &d.pp_governance_group); }

                self.proportional_count(p_th, d_th, &c.threshold)
            },
            GovernanceAction::HardForkInitiation(_) => 
                self.full_count(&p.hard_fork_initiation, &d.hard_fork_initiation, &c.threshold),
            GovernanceAction::TreasuryWithdrawals(_) => 
                self.proportional_count(zero, &d.treasury_withdrawal, &c.threshold),
            GovernanceAction::NoConfidence(_) => 
                self.proportional_count(&p.motion_no_confidence.clone(), &d.motion_no_confidence.clone(), zero),
            GovernanceAction::UpdateCommittee(_) => if thresholds.committee.is_empty() {
                self.proportional_count(&p.committee_no_confidence, &d.committee_no_confidence, zero)
            }
            else {
                self.proportional_count(&p.committee_normal, &d.committee_normal, zero)
            }
            GovernanceAction::NewConstitution(_) => self.proportional_count(zero, &d.update_constitution, &c.threshold),
            GovernanceAction::Information => self.proportional_count(one, one, zero)
        }
    }

    /// Checks whether action_id can be considered finally accepted
    fn is_finally_accepted(&self, action_id: &GovActionId) -> Result<bool> {
        let (_epoch, proposal) = self.proposals.get(action_id).ok_or_else(
            || anyhow!("action {} not found", action_id)
        )?;

        let conway_params = match &self.conway {
            None => return Err(anyhow!("Conway params are not known, cannot count votes")),
            Some(conway_params) => conway_params
        };

        let (d,p,c) = self.get_action_thresholds(proposal, conway_params)?;
        let (d_act, p_act, c_act) = self.get_actual_votes(action_id);
        Ok(d_act >= d && p_act >= p && c_act >= c)
    }

    /// Distribute and return all rewards for cast votes (government action is expired/voted)
    fn return_rewards(&self, _votes: Option<&HashMap<Voter, (DataHash, VotingProcedure)>>) {
    }

    /// Should be called when voting is over
    fn end_voting(&mut self, action_id: &GovActionId) {
        self.return_rewards(self.votes.get(&action_id));
        self.votes.remove(&action_id);
        self.proposals.remove(&action_id);
    }

    /// Returns actual votes: (Pool votes, DRep votes, committee votes)
    fn get_actual_votes(&self, action_id: &GovActionId) -> (u64, u64, u64) {
        let (mut p, mut d, mut c) = (0, 0, 0);
        if let Some(all_votes) = self.votes.get(&action_id) {
            for voter in all_votes.keys() {
                match voter {
                    Voter::ConstitutionalCommitteeKey(_) => c += 1,
                    Voter::ConstitutionalCommitteeScript(_) => c += 1,
                    Voter::DRepKey(key) => {
                        self.drep_stake.get(&DRepCredential::AddrKeyHash(key.clone())).inspect(|v| d += *v);
                    }
                    Voter::DRepScript(script) => {
                        self.drep_stake.get(&DRepCredential::ScriptHash(script.clone())).inspect(|v| d += *v);
                    }
                    Voter::StakePoolKey(pool) => {
                        self.spo_stake.get(pool).inspect(|v| p += *v);
                    }
                }
            }
        }
        (p, d, c)
    }

    /// Checks whether action is expired at the beginning of new_epoch
    fn is_expired(&self, new_epoch: u64, action_id: &GovActionId) -> Result<bool> {
        info!("Checking whether {} is expired at new epoch {}", action_id, new_epoch);
        let (proposal_epoch, _proposal) = self.proposals.get(action_id)
            .ok_or_else(|| anyhow!("action {} not found", action_id))?;

        Ok(proposal_epoch + self.get_conway_params()?.gov_action_lifetime as u64 <= new_epoch)
    }

    /// Checks and updates action_id state at the start of new_epoch
    fn process_one_proposal(&mut self, new_epoch: u64, action_id: &GovActionId) -> Result<()> {
        if self.is_finally_accepted(&action_id)? {
            self.end_voting(&action_id);
        }

        if self.is_expired(new_epoch, &action_id)? {
            self.end_voting(&action_id);
            info!("New epoch {new_epoch}: voting for {action_id} is expired");
        }

        Ok(())
    }

    /// Loops through all actions and checks their status for the new_epoch
    fn process_new_epoch(&mut self, new_epoch: u64) {
        let actions = self.proposals.keys().map(|a| a.clone()).collect::<Vec<_>>();

        for action_id in actions.iter() {
            info!("Epoch {}: processing action {}", new_epoch, action_id);
            if let Err(e) = self.process_one_proposal(new_epoch, &action_id) {
                error!("Error processing governance {action_id}: {e}");
            }
        }
    }

    async fn log_stats(&self) {
        info!("props: {}, props_with_id: {}, votes: {}, stored proposal procedures: {}, drep stake msgs,size: {},{}",
            self.proposal_count, self.action_proposal_count, self.votes_count, self.proposals.len(),
            self.drep_stake_messages_count, self.drep_stake.len()
        );

        for (action_id, procedure) in self.votes.iter() {
            info!("{}{} => {}",
                action_id,
                if self.proposals.contains_key(action_id) {""} else {" (absent)"},
                procedure.len()
            )
        }
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
                result.push((action.clone(), voter.clone(), transaction.clone(), voting_proc.clone()));
            }
        }
        Ok(result)
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }

    /// Implementation of new governance message processing handle
    async fn handle_impl(&mut self, block: &BlockInfo,
                         governance_message: &GovernanceProceduresMessage) -> Result<()> {
        if block.new_epoch {
            debug!("Processing new epoch {}", block.epoch);
            self.process_new_epoch(block.epoch);
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
                    error!("Error handling governance voting block {}, trans {}: '{}'",
                        block.number, trans.encode_hex::<String>(), e
                    );
                }
                self.votes_count += voter_votes.voting_procedures.len();
            }
        }
        Ok(())
    }
}
