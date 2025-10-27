use crate::voting_state::VotingRegistrationState;
use acropolis_common::protocol_params::ConwayParams;
use acropolis_common::{
    BlockInfo, DRepCredential, DelegatedStake, EnactStateElem, GovActionId, GovernanceAction,
    GovernanceOutcome, GovernanceOutcomeVariant, KeyHash, Lovelace, ProposalProcedure,
    SingleVoterVotes, TreasuryWithdrawalsAction, TxHash, Vote, Voter, VotesCount, VotingOutcome,
    VotingProcedure,
};
use anyhow::{anyhow, bail, Result};
use hex::ToHex;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::ops::Range;
use tracing::{debug, error, info};

#[derive(Debug)]
pub struct ActionStatus {
    voting_epochs: Range<u64>,
    ratification_epoch: Option<u64>,
    enactment_epoch: Option<u64>,
    expiration_epoch: Option<u64>,
}

impl ActionStatus {
    pub fn new(current_epoch: u64, voting_length: u64) -> Self {
        Self {
            voting_epochs: current_epoch..current_epoch + voting_length + 1,
            ratification_epoch: None,
            enactment_epoch: None,
            expiration_epoch: None,
        }
    }

    pub fn is_active(&self, current_epoch: u64) -> bool {
        self.voting_epochs.contains(&current_epoch)
    }

    pub fn is_accepted(&self) -> bool {
        self.ratification_epoch.is_some()
    }
}

pub struct ConwayVoting {
    conway: Option<ConwayParams>,
    bootstrap: Option<bool>,

    pub proposals: HashMap<GovActionId, (u64, ProposalProcedure)>,
    pub votes: HashMap<GovActionId, HashMap<Voter, (TxHash, VotingProcedure)>>,
    action_status: HashMap<GovActionId, ActionStatus>,

    verification_output_file: Option<String>,
    action_proposal_count: usize,
    votes_count: usize,
}

impl ConwayVoting {
    pub fn new(verification_output_file: Option<String>) -> Self {
        Self {
            conway: None,
            bootstrap: None,
            proposals: Default::default(),
            votes: Default::default(),
            action_status: Default::default(),
            action_proposal_count: 0,
            votes_count: 0,
            verification_output_file,
        }
    }

    pub fn get_conway_params(&self) -> Result<&ConwayParams> {
        self.conway.as_ref().ok_or_else(|| anyhow!("Conway parameters not available"))
    }

    /// Update Conway governance parameters.
    /// `bootstrap` parameter: Conway era is split into Chang era (protocol version 9.0)
    /// and Plomin era (10.0). During Chang era governance procedures are working in
    /// bootstrap (limited) mode.
    /// Pass true at Chang era, and false at Plomin era.
    /// https://docs.cardano.org/about-cardano/evolution/upgrades/chang
    pub fn update_parameters(&mut self, conway: &Option<ConwayParams>, bootstrap: bool) {
        self.conway = conway.clone();
        self.bootstrap = Some(bootstrap);
    }

    pub fn insert_proposal_procedure(
        &mut self,
        epoch: u64,
        proc: &ProposalProcedure,
    ) -> Result<()> {
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

        let prev = self.action_status.insert(
            proc.gov_action_id.clone(),
            ActionStatus::new(epoch, self.get_conway_params()?.gov_action_lifetime as u64),
        );
        if let Some(prev) = prev {
            return Err(anyhow!(
                "Governance procedure {} action status already exists! Old: {:?}",
                proc.gov_action_id,
                prev
            ));
        }

        Ok(())
    }

    /// Update votes memory cache
    pub fn insert_voting_procedure(
        &mut self,
        current_epoch: u64,
        voter: &Voter,
        transaction: &TxHash,
        voter_votes: &SingleVoterVotes,
    ) -> Result<()> {
        self.votes_count += voter_votes.voting_procedures.len();
        for (action_id, procedure) in voter_votes.voting_procedures.iter() {
            let votes = self.votes.entry(action_id.clone()).or_insert_with(|| HashMap::new());

            match self.action_status.get(action_id) {
                None => {
                    error!(
                        "Governance vote by {} for non-registered {}. Ignored.",
                        voter, action_id
                    );
                    continue;
                }
                Some(vs) if !vs.is_active(current_epoch) => {
                    error!(
                        "Governance vote by {} for inactive {}. Active at {:?}. Ignored.",
                        voter, action_id, vs.voting_epochs
                    );
                    continue;
                }
                Some(_) => (),
            }

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
        drep_stake: &HashMap<DRepCredential, Lovelace>,
        spo_stake: &HashMap<KeyHash, DelegatedStake>,
    ) -> Result<VotingOutcome> {
        let (_epoch, proposal) = self
            .proposals
            .get(action_id)
            .ok_or_else(|| anyhow!("action {} not found", action_id))?;
        let conway_params = self.get_conway_params()?;
        let bootstrap = self.bootstrap.ok_or_else(|| anyhow!("'bootstrap' param not set"))?;
        let threshold = voting_state.get_action_thresholds(proposal, conway_params, bootstrap)?;

        let votes = self.get_actual_votes(action_id, drep_stake, spo_stake);
        let voted = votes.majorizes(&threshold);
        let previous_ok = match proposal.gov_action.get_previous_action_id() {
            Some(act) => self.action_status.get(&act).map(|x| x.is_accepted()).unwrap_or(false),
            None => true,
        };
        let accepted = previous_ok && voted;
        info!(
            "Proposal {action_id}: votes {votes}, thresholds {threshold}, prevous_ok {previous_ok}, \
             voted {voted}, result {accepted}"
        );

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
    fn get_actual_votes(
        &self,
        action_id: &GovActionId,
        drep_stake: &HashMap<DRepCredential, Lovelace>,
        spo_stake: &HashMap<KeyHash, DelegatedStake>,
    ) -> VotesCount {
        let mut votes = VotesCount::zero();
        if let Some(all_votes) = self.votes.get(&action_id) {
            for (voter, (_hash, voting_proc)) in all_votes.iter() {
                if voting_proc.vote != Vote::Yes {
                    // TODO: correctly count abstain votes + count vote pools
                    continue;
                }

                match voter {
                    Voter::ConstitutionalCommitteeKey(_) => votes.committee += 1,
                    Voter::ConstitutionalCommitteeScript(_) => votes.committee += 1,
                    Voter::DRepKey(key) => {
                        drep_stake
                            .get(&DRepCredential::AddrKeyHash(key.clone()))
                            .inspect(|v| votes.drep += *v);
                    }
                    Voter::DRepScript(script) => {
                        drep_stake
                            .get(&DRepCredential::ScriptHash(script.clone()))
                            .inspect(|v| votes.drep += *v);
                    }
                    Voter::StakePoolKey(pool) => {
                        spo_stake.get(pool).inspect(|ds| votes.pool += ds.live);
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

        let action_status = self
            .action_status
            .get(action_id)
            .ok_or_else(|| anyhow!("Action status {action_id} not found"))?;

        Ok(!action_status.is_active(new_epoch))
    }

    fn pack_as_enact_state_elem(p: &ProposalProcedure) -> Option<EnactStateElem> {
        match &p.gov_action {
            GovernanceAction::Information => None,
            GovernanceAction::TreasuryWithdrawals(_wt) => None,
            GovernanceAction::HardForkInitiation(hf) => {
                Some(EnactStateElem::ProtVer(hf.protocol_version.clone()))
            }
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
        drep_stake: &HashMap<DRepCredential, Lovelace>,
        spo_stake: &HashMap<KeyHash, DelegatedStake>,
    ) -> Result<Option<VotingOutcome>> {
        let outcome = self.is_finally_accepted(voting_state, &action_id, drep_stake, spo_stake)?;
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

    fn gov_action_id_to_string(action_id: &GovActionId) -> String {
        format!(
            "\"transaction: {}, action_index: {}\"",
            hex::encode(action_id.transaction_id),
            action_id.action_index
        )
    }

    fn get_action_name(action: &GovernanceAction) -> &str {
        match action {
            GovernanceAction::ParameterChange(_) => "ParameterChange",
            GovernanceAction::HardForkInitiation(_) => "HardForkInitiation",
            GovernanceAction::TreasuryWithdrawals(_) => "TreasuryWithdrawals",
            GovernanceAction::NoConfidence(_) => "NoConfidence",
            GovernanceAction::UpdateCommittee(_) => "UpdateCommittee",
            GovernanceAction::NewConstitution(_) => "NewConstitution",
            GovernanceAction::Information => "Information",
        }
    }

    fn prepare_quotes(input: &str) -> String {
        input.replace("\"", "\"\"")
    }

    /// Function dumps information about completed (expired, ratified, enacted) governance
    /// actions in format, close to that of `gov_action_proposal` from `sqldb`.
    pub fn print_outcome_to_verify(&self, outcome: &Vec<GovernanceOutcome>) -> Result<()> {
        let out_file_name = match &self.verification_output_file {
            Some(o) => o,
            None => return Ok(()),
        };

        let mut out_file = match OpenOptions::new().append(true).open(out_file_name.clone()) {
            Ok(res) => res,
            Err(e) => bail!("Cannot open verification output {out_file_name} for writing: {e}"),
        };

        // If there is no outcome, the file will be created (appended), but not changed.
        // This is intentional for ease of debugging.
        for elem in outcome.iter() {
            let prev_action = match &elem.voting.procedure.gov_action.get_previous_action_id() {
                Some(act) => Self::gov_action_id_to_string(act),
                None => "".to_owned(),
            };

            let action_status =
                self.action_status.get(&elem.voting.procedure.gov_action_id).ok_or_else(|| {
                    anyhow!(
                        "Cannot get action status for {}",
                        &elem.voting.procedure.gov_action_id
                    )
                })?;

            let deposit = &elem.voting.procedure.deposit;
            let reward = hex::encode(elem.voting.procedure.reward_account.get_hash());
            let expire = action_status.voting_epochs.end;
            let ratification_info = if elem.voting.accepted {
                format!(
                    "{:?},{:?},,",
                    action_status.ratification_epoch, action_status.enactment_epoch
                )
            } else {
                format!(",,,{:?}", action_status.expiration_epoch)
            };
            let txid: String = elem.voting.procedure.gov_action_id.transaction_id.encode_hex();
            let idx = elem.voting.procedure.gov_action_id.action_index;
            let ptype = Self::get_action_name(&elem.voting.procedure.gov_action);
            let proc = Self::prepare_quotes(&format!("{:?}", &elem.voting.procedure.gov_action));
            let cast = &elem.voting.votes_cast;
            let threshold = &elem.voting.votes_threshold;

            // id,tx_id,index,prev_gov_action_proposal,deposit,return_address,expiration,
            // voting_anchor_id,type,description,param_proposal,ratified_epoch,enacted_epoch,
            // dropped_epoch,expired_epoch,votes_cast,votes_threshold
            let res = format!(
                "{},{txid},{idx},{prev_action},{deposit},{reward},{expire},,{ptype},\"{proc}\",,\
                 {ratification_info},{cast},{threshold}\n",
                elem.voting.procedure.gov_action_id
            );
            if let Err(e) = out_file.write(&res.as_bytes()) {
                error!(
                    "Cannot write 'res' to verification output {out_file_name} for writing: {e}"
                );
            }
        }

        Ok(())
    }

    pub fn finalize_conway_voting(
        &mut self,
        new_block: &BlockInfo,
        voting_state: &VotingRegistrationState,
        drep_stake: &HashMap<DRepCredential, Lovelace>,
        spo_stake: &HashMap<KeyHash, DelegatedStake>,
    ) -> Result<Vec<GovernanceOutcome>> {
        let mut outcome = Vec::<GovernanceOutcome>::new();
        let actions = self.proposals.keys().map(|a| a.clone()).collect::<Vec<_>>();

        for action_id in actions.iter() {
            info!(
                "Epoch {} started: processing action {}",
                new_block.epoch, action_id
            );
            let one_outcome = match self.process_one_proposal(
                new_block.epoch,
                &voting_state,
                &action_id,
                drep_stake,
                spo_stake,
            ) {
                Err(e) => {
                    error!("Error processing governance {action_id}: {e}");
                    continue;
                }
                Ok(None) => continue,
                Ok(Some(out)) if out.accepted => {
                    let mut action_to_perform = GovernanceOutcomeVariant::NoAction;

                    if let Some(elem) = Self::pack_as_enact_state_elem(&out.procedure) {
                        action_to_perform = GovernanceOutcomeVariant::EnactStateElem(elem);
                    } else if let Some(wt) = Self::retrieve_withdrawal(&out.procedure) {
                        action_to_perform = GovernanceOutcomeVariant::TreasuryWithdrawal(wt);
                    }

                    GovernanceOutcome {
                        voting: out,
                        action_to_perform,
                    }
                }
                Ok(Some(out)) => GovernanceOutcome {
                    voting: out,
                    action_to_perform: GovernanceOutcomeVariant::NoAction,
                },
            };

            outcome.push(one_outcome);
        }

        Ok(outcome)
    }

    pub fn log_conway_voting_stats(&self) {
        let mut proposal_procedures =
            self.proposals.keys().cloned().collect::<HashSet<GovActionId>>();

        for (action_id, voting_procedure) in self.votes.iter() {
            let proposal = match self.proposals.get(action_id) {
                None => " (absent) ".to_string(),
                Some(p) => {
                    proposal_procedures.remove(action_id);
                    format!(" {p:?} ")
                }
            };
            info!("{action_id}: {proposal} => {voting_procedure:?}",)
        }

        if !proposal_procedures.is_empty() {
            let pp = proposal_procedures.into_iter().map(|x| format!("{x},")).collect::<String>();
            info!("Proposal procedures without 'votes' records: [{}]", pp);
        }
    }

    /// Processes final `outcomes`, checks ratification/enaction epochs,
    /// updates `action_status` data structrure.
    pub fn update_action_status_with_outcomes(
        &mut self,
        epoch: u64,
        outcomes: &Vec<GovernanceOutcome>,
    ) -> Result<()> {
        for one_outcome in outcomes.iter() {
            let action_id = &one_outcome.voting.procedure.gov_action_id;
            let action = self
                .action_status
                .get_mut(action_id)
                .ok_or_else(|| anyhow!("Cannot get action status for {action_id}"))?;

            if one_outcome.voting.accepted {
                action.ratification_epoch = Some(epoch);
                action.enactment_epoch = Some(epoch + 1);
            } else {
                if action.is_active(epoch) {
                    bail!(
                        "Impossible outcome: {action_id} votes {:?}, not ended at {epoch}",
                        action.voting_epochs
                    );
                }
                action.expiration_epoch = Some(epoch);
            }
        }
        Ok(())
    }

    pub fn get_stats(&self) -> String {
        format!(
            "conway proposals: {}, conway votes: {}",
            self.proposals.len(),
            self.votes.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::Anchor;

    fn create_governance_outcome(id: u8) -> GovernanceOutcome {
        let votes = VotesCount {
            committee: 1,
            drep: 1,
            pool: 1,
        };

        let v = VotingOutcome {
            procedure: ProposalProcedure {
                deposit: 0,
                reward_account: vec![],
                gov_action_id: GovActionId {
                    transaction_id: [0; 32],
                    action_index: id,
                },
                gov_action: GovernanceAction::Information,
                anchor: Anchor {
                    url: "".to_owned(),
                    data_hash: Vec::new(),
                },
            },
            votes_cast: votes.clone(),
            votes_threshold: votes.clone(),
            accepted: true,
        };

        GovernanceOutcome {
            voting: v,
            action_to_perform: GovernanceOutcomeVariant::NoAction,
        }
    }

    #[test]
    fn test_outcomes_queue() -> Result<()> {
        let mut voting = ConwayVoting::new(None);
        let oc1 = create_governance_outcome(1);
        voting.action_status.insert(
            oc1.voting.procedure.gov_action_id.clone(),
            ActionStatus {
                voting_epochs: 0..4,
                ratification_epoch: None,
                enactment_epoch: None,
                expiration_epoch: None,
            },
        );

        voting.update_action_status_with_outcomes(0, &vec![])?;
        voting.update_action_status_with_outcomes(1, &vec![oc1.clone()])?;
        assert_eq!(
            voting
                .action_status
                .get(&oc1.voting.procedure.gov_action_id)
                .unwrap()
                .ratification_epoch,
            Some(1)
        );
        assert_eq!(
            voting.action_status.get(&oc1.voting.procedure.gov_action_id).unwrap().enactment_epoch,
            Some(2)
        );

        let oc2 = create_governance_outcome(2);
        voting.action_status.insert(
            oc2.voting.procedure.gov_action_id.clone(),
            ActionStatus {
                voting_epochs: 0..5,
                ratification_epoch: None,
                enactment_epoch: None,
                expiration_epoch: None,
            },
        );
        voting.update_action_status_with_outcomes(2, &vec![oc2.clone()])?;
        assert_eq!(
            voting
                .action_status
                .get(&oc2.voting.procedure.gov_action_id)
                .unwrap()
                .ratification_epoch,
            Some(2)
        );
        assert_eq!(
            voting.action_status.get(&oc2.voting.procedure.gov_action_id).unwrap().enactment_epoch,
            Some(3)
        );
        Ok(())
    }

    #[test]
    fn test_prepare_quotes() -> Result<()> {
        let x = "\"A\"\" lot (\"of\") quotes\"";
        let xx = ConwayVoting::prepare_quotes(x);
        assert_eq!(xx, "\"\"A\"\"\"\" lot (\"\"of\"\") quotes\"\"");
        Ok(())
    }
}
