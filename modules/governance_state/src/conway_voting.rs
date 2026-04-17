use crate::voting_state::{AggregatedVotes, AggregatedVotesOutcome, VotingRegistrationState};
use acropolis_common::{
    messages::GovernanceBootstrapMessage,
    protocol_params::ConwayParams,
    validation::{GovernanceValidationError, ValidationError, ValidationOutcomes},
    AddrKeyhash, BlockInfo, ConstitutionalCommitteeKeyHash, ConstitutionalCommitteeScriptHash,
    DRepCredential, DRepKeyHash, DRepScriptHash, DelegatedStake, DelegatedStakeDefaultVote,
    EnactStateElem, GovActionId, GovernanceAction, GovernanceOutcome, GovernanceOutcomeVariant,
    Lovelace, PoolId, ProposalProcedure, ScriptHash, SingleVoterVotes, TreasuryWithdrawalsAction,
    TxHash, Vote, VoteCount, VoteResult, Voter, VotingOutcome, VotingProcedure,
};
use anyhow::{anyhow, bail, Result};
use hex::ToHex;
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    fs::{File, OpenOptions},
    io::{BufReader, Read, Write},
    ops::Range,
    path::Path,
    str::FromStr,
};
use tracing::{debug, error, info, warn};

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
            voting_epochs: current_epoch..current_epoch + voting_length + 2,
            ratification_epoch: None,
            enactment_epoch: None,
            expiration_epoch: None,
        }
    }

    pub fn is_active(&self, at_epoch: u64) -> bool {
        self.voting_epochs.contains(&at_epoch)
    }

    pub fn is_accepted(&self) -> bool {
        self.ratification_epoch.is_some()
    }
}

#[derive(Default)]
pub struct CastVotes {
    /// Map: voter -> (vote, stake, is_default vote)
    /// SPO default vote is "No", such votes can be missing from this hashmap.
    votes: HashMap<Voter, (Vote, Lovelace, bool)>,
}

impl CastVotes {
    #[cfg(test)]
    fn get(&self, voter: &Voter) -> Option<&(Vote, Lovelace, bool)> {
        self.votes.get(voter)
    }

    fn reg_vote(&mut self, voter: &Voter, vote: Vote, stake: Lovelace, df: bool) -> Result<()> {
        if let Some((v, s, df)) = self.votes.insert(voter.clone(), (vote.clone(), stake, df)) {
            bail!(
                "{voter:?} vote already registered: {:?}, stake {}, default {}, new vote: {:?}, stake {}",
                v,
                s,
                df,
                vote,
                stake
            );
        }
        Ok(())
    }

    pub fn new_from_file(filename: &Path) -> Result<Self> {
        let f = File::open(filename)?;
        let mut reader = BufReader::new(f);

        Self::new_from_reader(&mut reader, filename)
    }

    /// Reads actual cast votes from a file, given in parameter `filename`.
    /// The file is specified in csv format:
    /// voter-type, voter-key-hash, vote, voted-stake
    pub fn new_from_reader<R: Read>(reader: &mut R, filename: &Path) -> Result<Self> {
        let mut reader = csv::ReaderBuilder::new().delimiter(b',').from_reader(reader);
        let mut res = Self::default();

        for (n, line) in reader.records().enumerate() {
            let split = line?.iter().map(|x| x.to_owned()).collect::<Vec<String>>();
            let [vtype, vhash, vote, vstake] = &split[..] else {
                bail!("Unexpected elements count at line {n}, file {filename:?}: {split:?}");
            };

            let (vote_proper, is_default) = match vote.as_str().strip_prefix("Default:") {
                Some(vp) => (vp, true),
                None => (vote.as_str(), false),
            };

            let vote = Vote::try_from(vote_proper)?;
            let vstake = vstake.parse::<Lovelace>()?;

            let voter = match vtype.as_str() {
                "DRK" => Voter::DRepKey(DRepKeyHash::from_str(vhash)?),
                "DRS" => Voter::DRepScript(DRepScriptHash::from_str(vhash)?),
                "SPO" => Voter::StakePoolKey(PoolId::from_str(vhash)?),
                "CCK" => Voter::ConstitutionalCommitteeKey(
                    ConstitutionalCommitteeKeyHash::from_str(vhash)?,
                ),
                "CCS" => Voter::ConstitutionalCommitteeScript(
                    ConstitutionalCommitteeScriptHash::from_str(vhash)?,
                ),
                x => bail!("Unknown record type '{x}' at line {n}, file {filename:?}"),
            };

            res.reg_vote(&voter, vote, vstake, is_default)?
        }

        Ok(res)
    }

    pub fn compare(&self, epoch: u64, action_id: &GovActionId, reference: &CastVotes) {
        let mut equal = true;
        let mut equal_drep_count = 0;
        let mut equal_pool_count = 0;
        let mut equal_cons_count = 0;
        let mut diff_cnt = 0;
        let mut comp_cnt = 0;
        let mut ref_cnt = 0;

        for (key, (v, s, df)) in self.votes.iter() {
            let dfs = if *df { ":default" } else { "" };
            match reference.votes.get(key) {
                None => {
                    warn!("{epoch}, {action_id}, {key:?}: computed {v:<7?}:{s:<12}{dfs} ---- reference (None)");
                    comp_cnt += 1;
                    equal = false;
                }
                Some((rv, rs, rdf)) if rv == v && rs == s && rdf == df => match key {
                    Voter::DRepKey(_) | Voter::DRepScript(_) => equal_drep_count += 1,
                    Voter::StakePoolKey(_) => equal_pool_count += 1,
                    Voter::ConstitutionalCommitteeKey(_)
                    | Voter::ConstitutionalCommitteeScript(_) => equal_cons_count += 1,
                },
                Some((rv, rs, rdf)) => {
                    let rdfs = if *rdf { ":default" } else { "" };
                    warn!("{epoch}, {action_id}, {key:?}: computed {v:<7?}:{s:<12}{dfs} ---- reference {rv:<7?}:{rs:<12}{rdfs}");
                    diff_cnt += 1;
                    equal = false;
                }
            }
        }

        for (key, (rv, rs, rdf)) in reference.votes.iter() {
            if !self.votes.contains_key(key) && (!rdf || *rv != Vote::No) {
                let rdfs = if *rdf { ":default" } else { "" };
                warn!("{epoch}, {action_id}, {key:?}: computed (None) ---- reference {rv:<7?}:{rs:<12}{rdfs}");
                ref_cnt += 1;
                equal = false;
            }
        }

        if !equal {
            warn!(
                "Votes validation failed: epoch {epoch}, action_id {action_id}, \
                equal votes p{equal_pool_count}:d{equal_drep_count}:c{equal_cons_count}, \
                different votes {diff_cnt}, computed alone {comp_cnt}, reference alone {ref_cnt}",
            );
        }
    }

    pub fn compute_votes(&self) -> VoteResult<VoteCount> {
        let mut votes = VoteResult::<VoteCount> {
            committee: VoteCount::zero(),
            drep: VoteCount::zero(),
            pool: VoteCount::zero(),
        };

        for (key, (vote, stake, _df)) in self.votes.iter() {
            match key {
                Voter::ConstitutionalCommitteeKey(_) | Voter::ConstitutionalCommitteeScript(_) => {
                    votes.committee.register_vote(vote, 1)
                }
                Voter::DRepKey(_) | Voter::DRepScript(_) => votes.drep.register_vote(vote, *stake),
                Voter::StakePoolKey(_) => votes.pool.register_vote(vote, *stake),
            }
        }

        votes
    }
}

#[derive(Default, Clone)]
pub struct ConwayVoting {
    conway: Option<ConwayParams>,
    bootstrap: Option<bool>,

    pub proposals: HashMap<GovActionId, (u64, ProposalProcedure)>,
    pub proposal_order: Vec<GovActionId>,
    pub pending_votes: HashMap<GovActionId, HashMap<Voter, (TxHash, VotingProcedure)>>,
    pub votes: HashMap<GovActionId, HashMap<Voter, (TxHash, VotingProcedure)>>,
    action_status: HashMap<GovActionId, ActionStatus>,

    verify_votes_files: Option<String>,
    verification_output_file: Option<String>,
    reference_aggregates: HashMap<(u64, GovActionId), (AggregatedVotes, AggregatedVotesOutcome)>,
    action_proposal_count: usize,
    votes_count: usize,
}

impl ConwayVoting {
    pub fn new(
        verification_output_file: Option<String>,
        verify_votes_files: Option<String>,
        verify_aggregates_file: Option<String>,
    ) -> Result<Self> {
        let reference_aggregates = match verify_aggregates_file {
            Some(file) => AggregatedVotes::get_from_file(Path::new(&file))?,
            None => HashMap::new(),
        };

        Ok(Self {
            verify_votes_files,
            verification_output_file,
            reference_aggregates,
            ..Default::default()
        })
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
    ) -> Result<()> {
        // Populate proposals and action_status
        for (proposed_epoch, proposal) in &msg.proposals {
            self.insert_proposal_procedure_impl(*proposed_epoch, proposal, voting_length)?
                .as_result()?;
            /*
            let action_id = proposal.gov_action_id.clone();

            // Insert proposal
            self.proposals.insert(action_id.clone(), (*proposed_epoch, proposal.clone()));

            // Create action status - calculate voting range from proposed epoch
            let action_status = ActionStatus::new(*proposed_epoch, voting_length);
            self.action_status.insert(action_id, action_status);

             */
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

        Ok(())
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

    fn insert_proposal_procedure_impl(
        &mut self,
        epoch: u64,
        proc: &ProposalProcedure,
        voting_length: u64,
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

        if let Some(last) = self.proposal_order.last() {
            let (max_epoch, _pp) =
                self.proposals.get(last).ok_or_else(|| anyhow!("Action {last} not found"))?;
            if epoch < *max_epoch {
                bail!(
                    "{} has proposed at {epoch}, but {last} is already at {max_epoch}",
                    proc.gov_action_id
                );
            }
        }

        self.proposal_order.push(proc.gov_action_id.clone());

        let prev = self.action_status.insert(
            proc.gov_action_id.clone(),
            ActionStatus::new(epoch, voting_length),
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

    pub fn insert_proposal_procedure(
        &mut self,
        epoch: u64,
        proc: &ProposalProcedure,
    ) -> Result<ValidationOutcomes> {
        self.insert_proposal_procedure_impl(
            epoch,
            proc,
            self.get_conway_params()?.gov_action_lifetime as u64,
        )
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
            let votes = self.pending_votes.entry(action_id.clone()).or_default();

            match self.action_status.get(action_id) {
                None => {
                    info!(
                        "Discarding vote {action_id}, {:?}, {voter} => {:?}",
                        voter, procedure.vote
                    );

                    outcomes.push(ValidationError::BadGovernance(
                        GovernanceValidationError::GovActionsDoNotExist {
                            action_id: vec![action_id.clone()],
                        },
                    ));
                }

                Some(vs) if !vs.is_active(current_epoch) => {
                    info!(
                        "Discarding vote {action_id}, {:?}, {voter} => {:?}",
                        voter, procedure.vote
                    );

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

    fn check_bootstrap(bootstrap: bool, proposal: &ProposalProcedure) -> Result<()> {
        if bootstrap {
            match proposal.gov_action {
                GovernanceAction::Information
                | GovernanceAction::TreasuryWithdrawals(_)
                | GovernanceAction::HardForkInitiation(_)
                | GovernanceAction::ParameterChange(_) => Ok(()),
                _ => bail!(
                    "Unexpected governance action in bootstrap mode: {:?}",
                    proposal.gov_action
                ),
            }
        } else {
            Ok(())
        }
    }

    /// New committee members validitiy check:
    /// Each member may be active committee_max_term_length epochs at most.
    /// So, if the committee valid from `enactment_epoch`, it may be valid at
    /// enactment_epoch + 0, enactment_epoch + 1, ..., enactment_epoch +
    /// committee_max_term_length - 1, and must be invalid (expired) at
    /// enactment_epoch + committee_max_term_length.
    fn check_committee_validity(
        enactment_epoch: u64,
        proposal: &ProposalProcedure,
        conway_params: &ConwayParams,
    ) -> Result<bool> {
        match &proposal.gov_action {
            GovernanceAction::UpdateCommittee(data) => {
                let change = &data.data.new_committee_members;
                Ok(change.iter().all(|(_member, term)| {
                    *term >= enactment_epoch
                        && *term < enactment_epoch + conway_params.committee_max_term_length as u64
                }))
            }
            _ => Ok(true),
        }
    }

    /// Checks whether `action_id` can be considered finally accepted;
    /// `new_epoch` --- will become enactment epoch; that is, action is enacted immediately.
    fn is_accepted(
        &self,
        new_epoch: u64,
        voting_state: &VotingRegistrationState,
        action_id: &GovActionId,
        votes_cast: &VoteResult<VoteCount>,
        aggregated_votes: &AggregatedVotes,
    ) -> Result<VotingOutcome> {
        let votes = aggregated_votes.votes_to_rationals()?;

        let (_epoch, proposal) = self
            .proposals
            .get(action_id)
            .ok_or_else(|| anyhow!("action {} not found", action_id))?;
        let conway_params = self.get_conway_params()?;
        let threshold = voting_state.get_action_thresholds(proposal, conway_params);

        let bootstrap = self.is_bootstrap()?;
        let voted = voting_state.compare_votes(bootstrap, &votes, &threshold)?;
        Self::check_bootstrap(bootstrap, proposal)?;
        let previous_ok = match proposal.gov_action.get_previous_action_id() {
            Some(act) => self.action_status.get(&act).map(|x| x.is_accepted()).unwrap_or(false),
            None => true,
        };
        let committee_ok = Self::check_committee_validity(new_epoch, proposal, conway_params)?;
        let accepted = previous_ok && committee_ok && voted;
        info!(
            "Proposal {action_id}: enactment epoch {new_epoch}, votes {votes}, \
             thresholds {threshold}, prevous_ok {previous_ok}, bootstrap {bootstrap}, \
             voted {voted}, committee {committee_ok}, result {accepted}"
        );

        Ok(VotingOutcome {
            procedure: proposal.clone(),
            votes_cast: Some(votes_cast.clone()),
            votes_threshold: Some(threshold),
            accepted,
        })
    }

    /// Should be called when `action_id` is either ratified, or expired.
    fn end_voting(&mut self, action_id: &GovActionId) {
        self.pending_votes.remove(action_id);
        self.votes.remove(action_id);
        self.proposals.remove(action_id);
        self.proposal_order.retain(|id| id != action_id);
    }

    /// Returns actual cast votes. Specific rules (how to treat registered, but not voted
    /// voters) are not applied at this stage.
    fn get_actual_votes(
        &self,
        action_id: &GovActionId,
        drep_stake: &HashMap<DRepCredential, Lovelace>,
        spo_stake: &HashMap<PoolId, DelegatedStake>,
        default_vote_list: &[(PoolId, DelegatedStakeDefaultVote)],
    ) -> Result<CastVotes> {
        let mut cast_votes = CastVotes::default();
        let mut voted = HashSet::<Voter>::new();

        let Some(all_votes) = self.votes.get(action_id) else {
            return Ok(cast_votes);
        };

        let Some((_, proposal)) = self.proposals.get(action_id) else {
            bail!("Proposal {action_id} not found");
        };

        for (voter, (_hash, voting_proc)) in all_votes.iter() {
            voted.insert(voter.clone());
            match &voter {
                Voter::ConstitutionalCommitteeKey(_) | Voter::ConstitutionalCommitteeScript(_) => {
                    cast_votes.reg_vote(voter, voting_proc.vote.clone(), 1, false)?
                }
                Voter::DRepKey(key) => {
                    let cred = &DRepCredential::AddrKeyHash(AddrKeyhash::from(key.into_inner()));
                    if let Some(stake) = drep_stake.get(cred) {
                        cast_votes.reg_vote(voter, voting_proc.vote.clone(), *stake, false)?;
                    }
                }
                Voter::DRepScript(script) => {
                    let cred = &DRepCredential::ScriptHash(ScriptHash::from(script.into_inner()));
                    if let Some(stake) = drep_stake.get(cred) {
                        cast_votes.reg_vote(voter, voting_proc.vote.clone(), *stake, false)?;
                    }
                }
                Voter::StakePoolKey(pool) => {
                    if let Some(stake) = spo_stake.get(pool) {
                        cast_votes.reg_vote(
                            voter,
                            voting_proc.vote.clone(),
                            stake.active,
                            false,
                        )?;
                    }
                }
            }
        }

        if !self.is_bootstrap()? {
            let no_conf = match proposal.gov_action {
                GovernanceAction::NoConfidence(_) => Vote::Yes,
                _ => Vote::No,
            };

            for (pool, default_vote) in default_vote_list.iter() {
                let voter = Voter::StakePoolKey(*pool);
                if voted.contains(&voter) {
                    continue;
                }

                let vote = match &default_vote {
                    DelegatedStakeDefaultVote::AlwaysNoConfidence => &no_conf,
                    DelegatedStakeDefaultVote::AlwaysAbstain => &Vote::Abstain,
                    DelegatedStakeDefaultVote::NoDefault => continue,
                };

                let Some(stake) = spo_stake.get(pool) else {
                    debug!("Pool {pool}: has no stake, although has default vote {vote:?}");
                    continue;
                };

                cast_votes.reg_vote(&voter, vote.clone(), stake.active, true)?;
            }
        }

        Ok(cast_votes)
    }

    /// Checks whether action is expired at the beginning of new_epoch
    pub fn is_expired(&self, new_epoch: u64, action_id: &GovActionId) -> Result<bool> {
        debug!(
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
    /// The `epoch_to_compare` is the current epoch from Haskell node *at the moment of votes
    /// calculation*. So, votes, computed during E, are done for Mark of E-2/E-1 border.
    fn apply_votes_pattern(
        &self,
        action_id: &GovActionId,
        epoch_to_compare: u64,
    ) -> Option<String> {
        let pattern = self.verify_votes_files.as_ref()?;
        let tx_hash = hex::encode(action_id.transaction_id)[0..8].to_string();
        let act_id = format!("{tx_hash}_{}", action_id.action_index);
        let applied = pattern
            .replace("{action_id}", &act_id)
            .replace("{epoch}", &epoch_to_compare.to_string());
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
        default_vote: &[(PoolId, DelegatedStakeDefaultVote)],
    ) -> Result<Option<VotingOutcome>> {
        let cast_votes = self.get_actual_votes(action_id, drep_stake, spo_stake, default_vote)?;

        // Checking ratification for new_epoch-1/new_epoch transition.
        // This computation is done in Haskell node at new_epoch-1 epoch, and related to
        // new_epoch-3/new_epoch-2 mark.
        if let Some(ref_file) = self.apply_votes_pattern(action_id, new_epoch - 1) {
            let ref_path = Path::new(&ref_file);
            if ref_path.exists() {
                debug!("Verifying {action_id:?} at epoch {new_epoch}: file '{ref_path:?}'...");
                let reference_votes = CastVotes::new_from_file(ref_path)?;
                cast_votes.compare(new_epoch, action_id, &reference_votes);
            } else {
                debug!("Verifying {action_id:?} at epoch {new_epoch}: file '{ref_path:?}' not found, skipping");
            }
        }

        let counted = cast_votes.compute_votes();
        let (_, proc) = self
            .proposals
            .get(action_id)
            .ok_or_else(|| anyhow!("Proposal {action_id} not found"))?;
        let aggregated = voting_state.aggregate_votes(proc, self.is_bootstrap()?, &counted)?;
        let outcome =
            self.is_accepted(new_epoch, voting_state, action_id, &counted, &aggregated)?;
        let expired = self.is_expired(new_epoch, action_id)?;
        let aggregated_outcome = AggregatedVotesOutcome::new(outcome.accepted, expired);

        if let Some((ref_v, ref_oc)) =
            self.reference_aggregates.get(&(new_epoch - 1, action_id.clone()))
        {
            if ref_v != &aggregated {
                error!("Verifying {action_id} at {new_epoch}: computed {aggregated} != reference {ref_v}");
            } else {
                debug!("Verifying {action_id} at {new_epoch}: votes {aggregated}, ok");
            }

            if ref_oc != &aggregated_outcome {
                error!("Verifying voting outcome: {action_id} at {new_epoch}: computed {aggregated_outcome} != reference {ref_oc}");
            } else {
                debug!("Verifying voting outcome: {action_id} at {new_epoch}: {ref_oc}, ok");
            }
        }

        Ok((outcome.accepted || expired).then_some(outcome))
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
            let cast =
                elem.voting.votes_cast.as_ref().map(|x| format!("{}", x)).unwrap_or_default();
            let threshold =
                elem.voting.votes_threshold.as_ref().map(|x| format!("{}", x)).unwrap_or_default();

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

    fn delay_ratification(proposal_procedure: &ProposalProcedure) -> bool {
        matches!(
            proposal_procedure.gov_action,
            GovernanceAction::HardForkInitiation(_)
                | GovernanceAction::UpdateCommittee(_)
                | GovernanceAction::NoConfidence(_)
                | GovernanceAction::NewConstitution(_)
        )
    }

    /// Removes finalized actions: proveded in `outcomes` list.
    /// TODO: currently removal of finalization is made inside update_action_status_with_outcomes,
    /// so that function receives side effect; this function was added to separate it from
    /// updating action logic. Refactoring is planned to use this function instead.
    #[allow(dead_code)]
    pub fn remove_finalized(&mut self, outcomes: &[GovernanceOutcome]) {
        for one_outcome in outcomes.iter() {
            let action_id = &one_outcome.voting.procedure.gov_action_id;
            self.end_voting(action_id);
        }
    }

    pub fn finalize_conway_voting(
        &mut self,
        new_block: &BlockInfo,
        voting_state: &VotingRegistrationState,
        drep_stake: &HashMap<DRepCredential, Lovelace>,
        spo_stake: &HashMap<PoolId, DelegatedStake>,
        default_vote: &[(PoolId, DelegatedStakeDefaultVote)],
    ) -> Result<Vec<GovernanceOutcome>> {
        let mut outcome = Vec::<GovernanceOutcome>::new();
        let mut delay_ratification = false;

        let proposals = self.proposal_order.clone();
        for action_id in proposals.iter() {
            debug!(
                "Epoch {} started: processing action {}; delaying {}",
                new_block.epoch, action_id, delay_ratification
            );

            if delay_ratification {
                if self.is_expired(new_block.epoch, action_id)? {
                    let (_, proc) = self
                        .proposals
                        .get(action_id)
                        .ok_or_else(|| anyhow!("Proposal {action_id} not found"))?
                        .clone();
                    outcome.push(GovernanceOutcome {
                        voting: VotingOutcome {
                            procedure: proc,
                            votes_cast: None,
                            votes_threshold: None,
                            accepted: false,
                        },
                        action_to_perform: GovernanceOutcomeVariant::NoAction,
                    });
                }
                continue;
            }

            let one_outcome = match self.process_one_proposal(
                new_block.epoch,
                voting_state,
                action_id,
                drep_stake,
                spo_stake,
                default_vote,
            )? {
                None => continue,
                Some(out) if out.accepted => {
                    let mut action_to_perform = GovernanceOutcomeVariant::NoAction;

                    if let Some(elem) = Self::pack_as_enact_state_elem(&out.procedure) {
                        action_to_perform = GovernanceOutcomeVariant::EnactStateElem(elem);
                    } else if let Some(wt) = Self::retrieve_withdrawal(&out.procedure) {
                        action_to_perform = GovernanceOutcomeVariant::TreasuryWithdrawal(wt);
                    }

                    anyhow::ensure!(!delay_ratification);
                    delay_ratification = Self::delay_ratification(&out.procedure);

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
            debug!("Epoch start {new_epoch}, {action_id}: {proposal} => {voting_procedure:?}",)
        }

        if !proposal_procedures.is_empty() {
            let pp = proposal_procedures.into_iter().map(|x| format!("{x},")).collect::<String>();
            debug!(
                "Proposal procedures at {new_epoch} without 'votes' records: [{}]",
                pp
            );
        }
    }

    /// Processes final `outcomes`, checks ratification/enaction epochs,
    /// updates `action_status` data structrure, removes finalized actions from
    /// other data structures.
    pub fn update_action_status_with_outcomes(
        &mut self,
        new_epoch: u64,
        outcomes: &[GovernanceOutcome],
    ) -> Result<()> {
        for one_outcome in outcomes.iter() {
            let action_id = &one_outcome.voting.procedure.gov_action_id;
            let action = self
                .action_status
                .get_mut(action_id)
                .ok_or_else(|| anyhow!("Cannot get action status for {action_id}"))?;

            if one_outcome.voting.accepted {
                action.ratification_epoch = Some(new_epoch - 1);
                action.enactment_epoch = Some(new_epoch);
            } else {
                if action.is_active(new_epoch) {
                    bail!(
                        "Impossible outcome: {action_id} votes {:?}, not ended at {new_epoch}",
                        action.voting_epochs
                    );
                }
                action.expiration_epoch = Some(new_epoch - 1);
            }

            self.end_voting(action_id);
        }
        Ok(())
    }

    pub fn include_pending_votes(&mut self) -> Result<()> {
        for (action_id, pending) in self.pending_votes.drain() {
            let votes = self.votes.entry(action_id).or_default();
            for (voter, (tx_hash, voting_proc)) in pending.into_iter() {
                votes.insert(voter, (tx_hash, voting_proc));
            }
        }
        anyhow::ensure!(self.pending_votes.is_empty());
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
    use acropolis_common::{
        Anchor, CommitteeChange, CommitteeCredential, StakeAddress, UpdateCommitteeAction,
    };

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
            votes_cast: Some(votes.clone()),
            votes_threshold: Some(votes_th.clone()),
            accepted,
        };

        GovernanceOutcome {
            voting: v,
            action_to_perform: GovernanceOutcomeVariant::NoAction,
        }
    }

    /// Simple test for general mechanics of action_status processing:
    /// Outcome, published at epoch E:
    /// * either expired at epoch E-1
    /// * or ratified at epoch E-1 and enacted at epoch E
    #[test]
    fn test_outcomes_action_status() -> Result<()> {
        let mut voting = ConwayVoting::new(None, None, None)?;
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
            Some(0)
        );
        assert_eq!(
            voting.action_status.get(&oc1.voting.procedure.gov_action_id).unwrap().enactment_epoch,
            Some(1)
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
            Some(4)
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
        let [_vtype, _vhash, _vote, _vstake] = &split[..] else {
            return Ok(());
        };
        bail!("Should not be able to decompose {split:?} into 4 elements");
    }

    #[test]
    fn test_new_from_file() -> Result<()> {
        let cast_votes = CastVotes::new_from_reader(
            &mut "#\"voter-type\", \"voter-key-hash\", \"vote\", \"stake\"\n\
            \"SPO\",\"0000fc522cea692e3e714b392d90cec75e4b87542c5f9638bf9a363a\",\"Yes\",37035968048975\n\
            \"DRK\",\"fcc1946fe92b7f27a8b21d6639bffc72be07157b2745ef204d7467c0\",\"No\",177029805388\n\
            \"DRS\",\"2bccc3b22a9d63fe5f85ea1f48536cc434ff17e8a8917111119159f4\",\"Abstain\",308757729830\n".as_bytes(),
            Path::new("")
        )?;

        assert_eq!(
            cast_votes.get(&Voter::StakePoolKey(PoolId::from_str(
                "0000fc522cea692e3e714b392d90cec75e4b87542c5f9638bf9a363a"
            )?)),
            Some(&(Vote::Yes, 37035968048975, false))
        );

        assert_eq!(
            cast_votes.get(&Voter::DRepKey(DRepKeyHash::from_str(
                "fcc1946fe92b7f27a8b21d6639bffc72be07157b2745ef204d7467c0"
            )?)),
            Some(&(Vote::No, 177029805388, false))
        );

        assert_eq!(
            cast_votes.get(&Voter::DRepScript(DRepScriptHash::from_str(
                "2bccc3b22a9d63fe5f85ea1f48536cc434ff17e8a8917111119159f4"
            )?)),
            Some(&(Vote::Abstain, 308757729830, false))
        );

        Ok(())
    }

    #[test]
    fn test_committee_validity() -> Result<()> {
        let proposal_procedure = ProposalProcedure {
            deposit: 0,
            reward_account: StakeAddress::default(),
            gov_action_id: GovActionId {
                transaction_id: TxHash::default(),
                action_index: 0,
            },
            gov_action: GovernanceAction::UpdateCommittee(UpdateCommitteeAction {
                previous_action_id: None,
                data: CommitteeChange {
                    removed_committee_members: Default::default(),
                    new_committee_members: HashMap::from([(
                        CommitteeCredential::AddrKeyHash(AddrKeyhash::default()),
                        726,
                    )]),
                    terms: Default::default(),
                },
            }),
            anchor: Anchor {
                url: "".to_owned(),
                data_hash: Vec::new(),
            },
        };

        let params = ConwayParams {
            committee_max_term_length: 146,
            ..Default::default()
        };
        assert!(!ConwayVoting::check_committee_validity(
            580,
            &proposal_procedure,
            &params
        )?);
        assert!(ConwayVoting::check_committee_validity(
            581,
            &proposal_procedure,
            &params
        )?);
        Ok(())
    }
}
