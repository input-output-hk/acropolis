use crate::voting_state::VotingRegistrationState;
use acropolis_common::messages::GovernanceBootstrapMessage;
use acropolis_common::protocol_params::ConwayParams;
use acropolis_common::validation::ValidationOutcomes;
use acropolis_common::validation::{GovernanceValidationError, ValidationError};
use acropolis_common::{
    AddrKeyhash, BlockInfo, Committee, CommitteeCredential, ConstitutionalCommitteeKeyHash, DRepScriptHash,
    ConstitutionalCommitteeScriptHash, DRepCredential, DRepKeyHash, DelegatedStake, EnactStateElem, GovActionId, GovernanceAction, GovernanceOutcome, GovernanceOutcomeVariant, Lovelace, PoolId, ProposalProcedure, ScriptHash, SingleVoterVotes, TreasuryWithdrawalsAction, TxHash, Vote, VoteCount, VoteResult, Voter, VotingOutcome, VotingProcedure};
use anyhow::{anyhow, bail, Result};
use hex::ToHex;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fs::{File, OpenOptions};
use std::hash::Hash;
use std::io::Write;
use std::ops::Range;
use std::path::Path;
use std::str::FromStr;
use tracing::{debug, error, info};

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Default)]
pub struct CastVotes {
    votes: HashMap<Voter, (Vote, Lovelace)>,
}

impl CastVotes {
    fn get(&self, voter: &Voter) -> Option<&(Vote, Lovelace)> {
        self.votes.get(voter)
    }

    fn reg_vote(&mut self, voter: &Voter, vote: Vote, stake: Lovelace) -> Result<()> {
        if let Some((v, s)) = self.votes.insert(voter.clone(), (vote.clone(), stake)) {
            bail!("{voter:?} vote already registered: {:?}, stake {}, new vote: {:?}, stake {}",
                v, s, vote, stake);
        }
        Ok(())
    }

    /// Reads actual cast votes from a file, given in parameter `filename`.
    /// The file is specified in csv format:
    /// voter-type, voter-key-hash, vote, voted-stake
    pub fn new_from_file(filename: &Path) -> Result<Self> {
        let reader = csv::ReaderBuilder::new().delimiter(b',').from_path(&filename);
        let mut res = Self::default();

        for (n,line) in reader?.records().enumerate() {
            println!("Line: {:?}", line);

            let split = line?.iter().map(|x| x.to_owned()).collect::<Vec<String>>();
            let [vtype, vhash, vote, vstake] = &split[..] else {
                bail!("Unexpected elements count at line {n}, file {filename:?}: {split:?}");
            };

            let vote = Vote::try_from(vote.as_str())?;
            let vstake = vstake.parse::<Lovelace>()?;

            match vtype.as_str() {
                "DRK" => res.reg_vote(
                    &Voter::DRepKey(DRepKeyHash::from_str(vhash)?), vote, vstake
                )?,
                "DRS" => res.reg_vote(
                    &Voter::DRepScript(DRepScriptHash::from_str(vhash)?), vote, vstake
                )?,
                "SPO" => res.reg_vote(
                    &Voter::StakePoolKey(PoolId::from_str(vhash)?), vote, vstake
                )?,
                "CCK" => res.reg_vote(
                    &Voter::ConstitutionalCommitteeKey(ConstitutionalCommitteeKeyHash::from_str(vhash)?), vote, vstake
                )?,
                "CCS" => res.reg_vote(
                    &Voter::ConstitutionalCommitteeScript(ConstitutionalCommitteeScriptHash::from_str(vhash)?), vote, vstake
                )?,
                x => bail!("Unknown record type '{x}' at line {n}, file {filename:?}")
            }
        }

        Ok(res)
    }

    pub fn compare(&self, epoch: u64, action_id: &GovActionId, reference: &CastVotes) {
        let mut equal = true;

        for (key, (v, s)) in self.votes.iter() {
            match reference.votes.get(key) {
                None => {
                    error!("{epoch}, {action_id}, {key:?}: computed {v:<7?}: {s:<12} ---- reference (None)");
                    equal = false;
                },
                Some((rv, rs)) if rv == v && rs == s => (),
                Some((rv, rs)) => {
                    error!("{epoch}, {action_id}, {key:?}: computed {v:<7?}:{s:<12} ---- reference {rv:<7?}:{rs:<12}");
                    equal = false;
                }
            }
        }

        for (key, (rv, rs)) in reference.votes.iter() {
            if !self.votes.contains_key(key) {
                error!("{epoch}, {action_id}, {key:?}: computed (None) ---- reference {rv:<7?}:{rs:<12}");
                equal = false;
            }
        }

        if !equal {
            error!("Votes validation failed: epoch {epoch}, action_id {action_id}");
        }
    }

    pub fn compute_votes(&self) -> VoteResult<VoteCount> {
        let mut votes = VoteResult::<VoteCount> {
            committee: VoteCount::zero(),
            drep: VoteCount::zero(),
            pool: VoteCount::zero(),
        };

        for (_drep, (vote, stake)) in self.votes.iter() {
            votes.drep.register_vote(vote, *stake);
        }

        votes
    }
}

pub struct ConwayVoting {
    conway: Option<ConwayParams>,
    bootstrap: Option<bool>,

    pub proposals: HashMap<GovActionId, (u64, ProposalProcedure)>,
    pub votes: HashMap<GovActionId, HashMap<Voter, (TxHash, VotingProcedure)>>,
    action_status: HashMap<GovActionId, ActionStatus>,

    verification_votes_pattern: Option<String>,
    verification_output_file: Option<String>,
    action_proposal_count: usize,
    votes_count: usize,
}

impl ConwayVoting {
    pub fn new(
        verification_votes_pattern: Option<String>,
        verification_output_file: Option<String>
    ) -> Self {
        Self {
            conway: None,
            bootstrap: None,
            proposals: Default::default(),
            votes: Default::default(),
            action_status: Default::default(),
            action_proposal_count: 0,
            votes_count: 0,
            verification_votes_pattern,
            verification_output_file,
        }
    }

    pub fn is_bootstrap(&self) -> Result<bool> {
        self.bootstrap.ok_or_else(|| anyhow!("ConwayVoting::is_bootstrap is not set"))
    }

    /// Bootstrap the governance state from a snapshot
    /// Populates proposals, votes, and action_status from the bootstrap message
    pub fn bootstrap_from_snapshot(
        &mut self,
        msg: &GovernanceBootstrapMessage,
        voting_length: u64,
    ) {
        // Populate proposals and action_status
        for (proposed_epoch, proposal) in &msg.proposals {
            let action_id = proposal.gov_action_id.clone();

            // Insert proposal
            self.proposals.insert(action_id.clone(), (*proposed_epoch, proposal.clone()));

            // Create action status - calculate voting range from proposed epoch
            let action_status = ActionStatus::new(*proposed_epoch, voting_length);
            self.action_status.insert(action_id, action_status);
        }

        // Populate votes - convert from VotingProcedure to (TxHash, VotingProcedure)
        // Note: We don't have the original TxHash from the snapshot, so we use a placeholder
        let placeholder_tx = TxHash::default();
        for (action_id, voter_votes) in &msg.votes {
            let votes_entry = self.votes.entry(action_id.clone()).or_default();
            for (voter, procedure) in voter_votes {
                votes_entry.insert(voter.clone(), (placeholder_tx, procedure.clone()));
            }
        }

        tracing::info!(
            "ConwayVoting bootstrapped: {} proposals, {} actions with votes",
            self.proposals.len(),
            self.votes.len()
        );
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
    ) -> Result<ValidationOutcomes> {
        let mut outcomes = ValidationOutcomes::new();
        self.action_proposal_count += 1;
        let prev = self.proposals.insert(proc.gov_action_id.clone(), (epoch, proc.clone()));
        if let Some(prev) = prev {
            outcomes.push_anyhow(anyhow!(
                "Governance procedure {} already exists! New: {:?}, old: {:?}",
                proc.gov_action_id,
                (epoch, proc),
                prev
            ));
            return Ok(outcomes);
        }

        let prev = self.action_status.insert(
            proc.gov_action_id.clone(),
            ActionStatus::new(epoch, self.get_conway_params()?.gov_action_lifetime as u64),
        );

        if let Some(prev) = prev {
            outcomes.push_anyhow(anyhow!(
                "Governance procedure {} action status already exists! Old: {:?}",
                proc.gov_action_id,
                prev
            ));
        }

        Ok(outcomes)
    }

    /// Update votes memory cache
    pub fn insert_voting_procedure(
        &mut self,
        current_epoch: u64,
        voter: &Voter,
        transaction: &TxHash,
        voter_votes: &SingleVoterVotes,
    ) -> Result<ValidationOutcomes> {
        self.votes_count += voter_votes.voting_procedures.len();
        let mut outcomes = ValidationOutcomes::new();
        for (action_id, procedure) in voter_votes.voting_procedures.iter() {
            let votes = self.votes.entry(action_id.clone()).or_default();

            match self.action_status.get(action_id) {
                None => {
                    outcomes.push(ValidationError::BadGovernance(
                        GovernanceValidationError::GovActionsDoNotExist {
                            action_id: vec![action_id.clone()],
                        },
                    ));
                }

                Some(vs) if !vs.is_active(current_epoch) => {
                    outcomes.push(ValidationError::BadGovernance(
                        GovernanceValidationError::VotingOnExpiredGovAction(vec![(
                            voter.clone(),
                            action_id.clone(),
                        )]),
                    ));
                }

                Some(_) => {
                    if let Some((prev_trans, prev_vote)) =
                        votes.insert(voter.clone(), (*transaction, procedure.clone()))
                    {
                        // Re-voting is allowed; new vote must be treated as the proper one,
                        // older is to be discarded.
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!(
                                "Governance vote by {} for {} already registered! \
                                New: {:?}, old: {:?} from {}",
                                voter,
                                action_id,
                                procedure,
                                prev_vote,
                                prev_trans.encode_hex::<String>()
                            );
                        }
                    }
                }
            }
        }
        Ok(outcomes)
    }

    /// Checks whether action_id can be considered finally accepted
    fn is_finally_accepted(
        &self,
        new_epoch: u64,
        voting_state: &VotingRegistrationState,
        action_id: &GovActionId,
        cast_votes: &CastVotes,
    ) -> Result<VotingOutcome> {
        let votes = cast_votes.compute_votes();

        let (_epoch, proposal) = self
            .proposals
            .get(action_id)
            .ok_or_else(|| anyhow!("action {} not found", action_id))?;
        let conway_params = self.get_conway_params()?;
        let threshold = voting_state.get_action_thresholds(proposal, conway_params);

        let bootstrap = self.is_bootstrap()?;
        let voted = voting_state.compare_votes(proposal, bootstrap, &votes, &threshold)?;
        let previous_ok = match proposal.gov_action.get_previous_action_id() {
            Some(act) => self.action_status.get(&act).map(|x| x.is_accepted()).unwrap_or(false),
            None => true,
        };
        let accepted = previous_ok && voted;
        info!(
            "Proposal {action_id}: new epoch {new_epoch}, votes {votes}, thresholds {threshold}, prevous_ok {previous_ok}, \
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
    fn end_voting(&mut self, action_id: &GovActionId) {
        self.votes.remove(action_id);
        self.proposals.remove(action_id);
    }

    /// Returns actual cast votes. Specific rules (how to treat registered, but not voted
    /// voters) are not applied at this stage.
    fn get_actual_votes(
        &self,
        action_id: &GovActionId,
        drep_stake: &HashMap<DRepCredential, Lovelace>,
        spo_stake: &HashMap<PoolId, DelegatedStake>,
    ) -> Result<CastVotes> {
        let mut cast_votes = CastVotes::default();

        let Some(all_votes) = self.votes.get(action_id) else {
            return Ok(cast_votes);
        };

        for (voter, (_hash, voting_proc)) in all_votes.iter() {
            match &voter {
                Voter::ConstitutionalCommitteeKey(_) |
                Voter::ConstitutionalCommitteeScript(_) =>
                    cast_votes.reg_vote(voter, voting_proc.vote.clone(), 1)?,
                Voter::DRepKey(key) => {
                    let cred = &DRepCredential::AddrKeyHash(AddrKeyhash::from(
                        key.into_inner(),
                    ));
                    if let Some(stake) = drep_stake.get(cred) {
                        cast_votes.reg_vote(voter, voting_proc.vote.clone(), *stake)?;
                    }
                }
                Voter::DRepScript(script) => {
                    let cred = &DRepCredential::ScriptHash(ScriptHash::from(
                        script.into_inner(),
                    ));
                    if let Some(stake) = drep_stake.get(cred) {
                        cast_votes.reg_vote(voter, voting_proc.vote.clone(), *stake)?;
                    }
                }
                Voter::StakePoolKey(pool) => {
                    if let Some(stake) = spo_stake.get(pool) {
                        cast_votes.reg_vote(voter, voting_proc.vote.clone(), stake.active)?;
                    }
                }
            }
        }

        Ok(cast_votes)
    }

    /// Checks whether action is expired at the beginning of new_epoch
    pub fn is_expired(&self, new_epoch: u64, action_id: &GovActionId) -> Result<bool> {
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

    /// Replaces {action_id} with first 8 characters of transaction_id in hex and
    /// action_id.action_index, and {epoch} with epoch number.
    fn apply_votes_pattern(&self, action_id: &GovActionId, new_epoch: u64) -> Option<String> {
        let pattern = self.verification_votes_pattern.as_ref()?;
        let tx_hash = hex::encode(action_id.transaction_id)[0..8].to_string();
        let act_id = format!("{tx_hash}_{action_id}");
        let applied = pattern.replace("{action_id}", &act_id)
            .replace("{epoch}", &new_epoch.to_string());
        Some(applied)
    }

    /// Checks and updates action_id state at the start of new_epoch
    /// If the action is accepted, returns accepted ProposalProcedure.
    pub fn process_one_proposal(
        &mut self,
        new_epoch: u64,
        voting_state: &VotingRegistrationState,
        action_id: &GovActionId,
        drep_stake: &HashMap<DRepCredential, Lovelace>,
        spo_stake: &HashMap<PoolId, DelegatedStake>,
    ) -> Result<Option<VotingOutcome>> {
        let cast_votes = self.get_actual_votes(action_id, drep_stake, spo_stake)?;

        if let Some(ref_file) = self.apply_votes_pattern(action_id, new_epoch) {
            let ref_path = Path::new(&ref_file);
            if ref_path.exists() {
                let reference_votes = CastVotes::new_from_file(ref_path)?;
                cast_votes.compare(new_epoch, action_id, &reference_votes);
            }
        }

        let outcome =
            self.is_finally_accepted(new_epoch, voting_state, action_id, &cast_votes)?;

        let expired = self.is_expired(new_epoch, action_id)?;
        if outcome.accepted || expired {
            self.end_voting(action_id);
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

    fn prepare_quotes(input: &str) -> String {
        input.replace("\"", "\"\"")
    }

    /// Function dumps information about completed (expired, ratified, enacted) governance
    /// actions in format, close to that of `gov_action_proposal` from `sqldb`.
    pub fn print_outcome_to_verify(&self, outcome: &[GovernanceOutcome]) -> Result<()> {
        let out_file_name = match &self.verification_output_file {
            Some(o) => o,
            None => return Ok(()),
        };

        if !Path::new(&out_file_name).exists() {
            File::create(out_file_name)
                .map_err(|e| anyhow::anyhow!("Cannot create {out_file_name}: {e}"))?;
        }

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
            let start = action_status.voting_epochs.start;
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
            let ptype = elem.voting.procedure.gov_action.get_action_name();
            let prop_procedure = serde_json::to_string(&elem.voting.procedure)?;
            let proc = Self::prepare_quotes(&prop_procedure);
            let cast = &elem.voting.votes_cast;
            let threshold = &elem.voting.votes_threshold;

            // id,tx_id,index,prev_gov_action_proposal,deposit,return_address,start,
            // voting_anchor_id,type,description,param_proposal,ratified_epoch,enacted_epoch,
            // dropped_epoch,expired_epoch,votes_cast,votes_threshold
            let res = format!(
                "{},{txid},{idx},{prev_action},{deposit},{reward},{start},,{ptype},\"{proc}\",,\
                 {ratification_info},{cast},{threshold}\n",
                elem.voting.procedure.gov_action_id
            );
            if let Err(e) = out_file.write(res.as_bytes()) {
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
        spo_stake: &HashMap<PoolId, DelegatedStake>,
        vld: &mut ValidationOutcomes,
    ) -> Result<Vec<GovernanceOutcome>> {
        let mut outcome = Vec::<GovernanceOutcome>::new();
        let actions = self.proposals.keys().cloned().collect::<Vec<_>>();

        for action_id in actions.iter() {
            info!(
                "Epoch {} started: processing action {}",
                new_block.epoch, action_id
            );
            let one_outcome = match self.process_one_proposal(
                new_block.epoch,
                voting_state,
                action_id,
                drep_stake,
                spo_stake,
            )? {
                None => continue,
                Some(out) if out.accepted => {
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
                Some(out) => GovernanceOutcome {
                    voting: out,
                    action_to_perform: GovernanceOutcomeVariant::NoAction,
                },
            };

            outcome.push(one_outcome);
        }

        Ok(outcome)
    }

    pub fn log_conway_voting_stats(&self, new_epoch: u64) {
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
            info!("Epoch start {new_epoch}, {action_id}: {proposal} => {voting_procedure:?}",)
        }

        if !proposal_procedures.is_empty() {
            let pp = proposal_procedures.into_iter().map(|x| format!("{x},")).collect::<String>();
            info!(
                "Proposal procedures at {new_epoch} without 'votes' records: [{}]",
                pp
            );
        }
    }

    /// Processes final `outcomes`, checks ratification/enaction epochs,
    /// updates `action_status` data structrure.
    pub fn update_action_status_with_outcomes(
        &mut self,
        epoch: u64,
        outcomes: &[GovernanceOutcome],
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
    use acropolis_common::rational_number::RationalNumber;
    use acropolis_common::{Anchor, StakeAddress};

    fn create_governance_outcome(id: u8, accepted: bool) -> GovernanceOutcome {
        let votes = VoteResult::<VoteCount> {
            committee: VoteCount::zero(),
            drep: VoteCount::zero(),
            pool: VoteCount::zero(),
        };

        let votes_th = VoteResult::<RationalNumber> {
            committee: RationalNumber::ONE,
            drep: RationalNumber::ONE,
            pool: RationalNumber::ONE,
        };

        let v = VotingOutcome {
            procedure: ProposalProcedure {
                deposit: 0,
                reward_account: StakeAddress::default(),
                gov_action_id: GovActionId {
                    transaction_id: TxHash::default(),
                    action_index: id,
                },
                gov_action: GovernanceAction::Information,
                anchor: Anchor {
                    url: "".to_owned(),
                    data_hash: Vec::new(),
                },
            },
            votes_cast: votes.clone(),
            votes_threshold: votes_th.clone(),
            accepted,
        };

        GovernanceOutcome {
            voting: v,
            action_to_perform: GovernanceOutcomeVariant::NoAction,
        }
    }

    /// Simple test for general mechanics of action_status processing.
    #[test]
    fn test_outcomes_action_status() -> Result<()> {
        let mut voting = ConwayVoting::new(None, None);
        let oc1 = create_governance_outcome(1, true);
        voting.action_status.insert(
            oc1.voting.procedure.gov_action_id.clone(),
            ActionStatus {
                voting_epochs: 0..4,
                ratification_epoch: None,
                enactment_epoch: None,
                expiration_epoch: None,
            },
        );

        voting.update_action_status_with_outcomes(0, &[])?;
        voting.update_action_status_with_outcomes(1, std::slice::from_ref(&oc1))?;
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

        let oc2 = create_governance_outcome(2, false);
        let as2 = ActionStatus {
            voting_epochs: 0..5,
            ratification_epoch: None,
            enactment_epoch: None,
            expiration_epoch: None,
        };
        voting.action_status.insert(oc2.voting.procedure.gov_action_id.clone(), as2.clone());
        match voting.update_action_status_with_outcomes(2, std::slice::from_ref(&oc2)) {
            Err(e) => assert_eq!(
                e.to_string(),
                "Impossible outcome: gov_action1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq\
                     qqqqqqqqqy9ddhkc votes 0..5, not ended at 2"
                    .to_string()
            ),
            Ok(()) => panic!("Action should not be successful."),
        }
        assert_eq!(
            *voting.action_status.get(&oc2.voting.procedure.gov_action_id).unwrap(),
            as2
        );
        voting.update_action_status_with_outcomes(5, std::slice::from_ref(&oc2))?;
        assert_eq!(
            voting.action_status.get(&oc2.voting.procedure.gov_action_id).unwrap().expiration_epoch,
            Some(5)
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

    #[test]
    fn test_splitting_csv_string() -> Result<()> {
        let line = "\"DRK\",\"hash1\",\"Yes\",\"1000\"";
        let split = line.split(',').map(|x| x.to_owned()).collect::<Vec<String>>();
        let [vtype, vhash, vote, vstake] = &split[..] else {
            bail!("Unexpected elements count at line: {split:?}");
        };
        assert_eq!(vtype, "\"DRK\"");
        assert_eq!(vhash, "\"hash1\"");
        assert_eq!(vote, "\"Yes\"");
        assert_eq!(vstake, "\"1000\"");

        let split = vec!["a", "b", "c", "d", "e"];
        let [vtype, vhash, vote, vstake] = &split[..] else {
            return Ok(())
        };
        bail!("Should not be able to decompose {split:?} into 4 elements");
    }

    #[test]
    fn test_new_from_file() -> Result<()> {
        let filename = "../../modules/governance_state/data/test_votes.csv";

        let cast_votes = CastVotes::new_from_file(Path::new(filename))?;
        //#voter-type, voter-key-hash, vote, voted-stake
        //"SPO","0000fc522cea692e3e714b392d90cec75e4b87542c5f9638bf9a363a",Yes,37035968048975
        //"DRK","fcc1946fe92b7f27a8b21d6639bffc72be07157b2745ef204d7467c0",No,177029805388
        //"DRS","2bccc3b22a9d63fe5f85ea1f48536cc434ff17e8a8917111119159f4",Abstain,308757729830

        assert_eq!(
            cast_votes.get(&Voter::StakePoolKey(PoolId::from_str(
            "0000fc522cea692e3e714b392d90cec75e4b87542c5f9638bf9a363a")?
            )),
            Some(&(Vote::Yes, 37035968048975))
        );

        assert_eq!(
            cast_votes.get(&Voter::DRepKey(DRepKeyHash::from_str(
                "fcc1946fe92b7f27a8b21d6639bffc72be07157b2745ef204d7467c0"
            )?)),
            Some(&(Vote::No, 177029805388))
        );

        assert_eq!(
            cast_votes.get(&Voter::DRepScript(DRepScriptHash::from_str(
                "2bccc3b22a9d63fe5f85ea1f48536cc434ff17e8a8917111119159f4"
            )?)),
            Some(&(Vote::Abstain, 308757729830))
        );

        Ok(())
    }
}
