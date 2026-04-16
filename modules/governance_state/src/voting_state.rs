use acropolis_common::{
    protocol_params::ConwayParams, rational_number::RationalNumber, GovActionId, GovernanceAction,
    ProposalProcedure, ProtocolParamType, ProtocolParamUpdate, VoteCount, VoteResult,
};
use anyhow::{bail, Result};
use std::{
    cmp::max,
    collections::HashMap,
    fmt,
    fmt::Display,
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct VotingRegistrationState {
    /// Total stake in active voting SPOs stake
    registered_spos: u64,

    /// Total stake in registered DReps (not counting NoConfidence and Abstain DReps).
    registered_dreps: u64,

    /// No confidence DReps stake (not counted in `registered_dreps`)
    no_confidence_dreps: u64,

    /// Abstain DReps stake (not counted in `registered_dreps`)
    abstain_dreps: u64,

    /// Number of committee members (0 is treated as no committee; that is, no valid committee
    /// vote can pass if this value is 0).
    committee_size: u64,
}

impl fmt::Display for VotingRegistrationState {
    fn fmt(&self, res: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            res,
            "spos reg. {}, dreps {} (no-confidence {}, abstain {}), committee {}",
            self.registered_spos,
            self.registered_dreps,
            self.no_confidence_dreps,
            self.abstain_dreps,
            self.committee_size
        )
    }
}

/// Intermediate structure for votes computing. A bit strange, in order to be exact copy of
/// Haskell algorithm.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AggregatedVotes {
    committee_yes: u64,
    committee_without_abstain: u64,
    drep_yes: u64,
    drep_without_abstain: u64,
    spo_yes: u64,
    spo_active: u64,
    spo_abstain: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub enum AggregatedVotesOutcome {
    Ratified,
    Expired,
    NoOutcome,
}

impl VotingRegistrationState {
    // At least one vote in each category is enough.
    pub fn new(
        registered_spos: u64,
        registered_dreps: u64,
        no_confidence_dreps: u64,
        abstain_dreps: u64,
        committee_size: u64,
    ) -> Self {
        Self {
            registered_spos,
            registered_dreps,
            no_confidence_dreps,
            abstain_dreps,
            committee_size,
        }
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
            || p.coins_per_utxo_byte.is_some()
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
            || p.coins_per_utxo_byte.is_some()
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
    /// total corresponding votes count): (Pool, DRep, Committee).
    /// This function computes results in full governance implementation ('plomin' sub-era).
    pub fn get_action_thresholds(
        &self,
        pp: &ProposalProcedure,
        thresholds: &ConwayParams,
    ) -> VoteResult<RationalNumber> {
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

                VoteResult::new(c.threshold.clone(), d_th.clone(), p_th.clone())
            }
            GovernanceAction::HardForkInitiation(_) => VoteResult::new(
                c.threshold.clone(),
                d.hard_fork_initiation.clone(),
                p.hard_fork_initiation.clone(),
            ),
            GovernanceAction::TreasuryWithdrawals(_) => VoteResult::new(
                c.threshold.clone(),
                d.treasury_withdrawal.clone(),
                zero.clone(),
            ),
            GovernanceAction::NoConfidence(_) => VoteResult::new(
                zero.clone(),
                d.motion_no_confidence.clone(),
                p.motion_no_confidence.clone(),
            ),
            GovernanceAction::UpdateCommittee(_) => {
                if thresholds.committee.is_empty() {
                    VoteResult::new(
                        zero.clone(),
                        d.committee_no_confidence.clone(),
                        p.committee_no_confidence.clone(),
                    )
                } else {
                    VoteResult::new(
                        zero.clone(),
                        d.committee_normal.clone(),
                        p.committee_normal.clone(),
                    )
                }
            }
            GovernanceAction::NewConstitution(_) => VoteResult::new(
                c.threshold.clone(),
                d.update_constitution.clone(),
                zero.clone(),
            ),
            GovernanceAction::Information => {
                VoteResult::new(zero.clone(), one.clone(), one.clone())
            }
        }
    }

    /// Aggregates vote numbers (according to Haskell node implementation).
    /// `proc` --- proposal procedure being voted;
    /// `bootstrap` --- whether the vote is in bootstrap period;
    /// `votes` --- actual votes cast (yes/no/abstain) for each category (committee, DReps, SPOs).
    pub fn aggregate_votes(
        &self,
        proc: &ProposalProcedure,
        _bootstrap: bool,
        votes: &VoteResult<VoteCount>,
    ) -> Result<AggregatedVotes> {
        // Only 'info', 'hardfork' and 'parameter change' actions are allowed in bootstrap period
        // committee vote thresholds
        if votes.committee.total() > self.committee_size {
            bail!(
                "Committee vote count {} > committee size {}",
                votes.committee.total(),
                self.committee_size
            );
        }

        let mut aggregated = AggregatedVotes {
            committee_yes: votes.committee.yes,
            committee_without_abstain: self.committee_size - votes.committee.abstain,
            ..Default::default()
        };

        // DRep votes
        let total_dreps = self.registered_dreps + self.abstain_dreps + self.no_confidence_dreps;
        let mut non_voted = VoteCount::zero();
        non_voted.abstain = self.abstain_dreps;

        // Any DRep, which did not vote and has no default vote, is considered voting 'No'
        non_voted.no = self.registered_dreps - votes.drep.total();

        if let GovernanceAction::NoConfidence(_) = &proc.gov_action {
            non_voted.yes += self.no_confidence_dreps
        } else {
            non_voted.no += self.no_confidence_dreps;
        };

        if non_voted.total() + votes.drep.total() != total_dreps {
            bail!("Total votes (including votes from non-voted) != total dreps stake");
        }

        aggregated.drep_yes = votes.drep.yes + non_voted.yes;
        aggregated.drep_without_abstain = aggregated.drep_yes + votes.drep.no + non_voted.no;

        // SPO votes
        aggregated.spo_yes = votes.pool.yes;
        aggregated.spo_active = self.registered_spos;
        aggregated.spo_abstain = votes.pool.abstain;

        Ok(aggregated)
    }

    pub fn compare_votes(
        &self,
        bootstrap: bool,
        votes: &VoteResult<RationalNumber>,
        threshold: &VoteResult<RationalNumber>,
    ) -> Result<bool> {
        Ok(votes.committee >= threshold.committee
            && (bootstrap || votes.drep >= threshold.drep) // dreps ignored at bootstrap
            && votes.pool >= threshold.pool)
    }
}

impl Display for AggregatedVotes {
    fn fmt(&self, res: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            res,
            "cy{}/w{}:dy{}/w{}:py{}/t{}/a{}",
            self.committee_yes,
            self.committee_without_abstain,
            self.drep_yes,
            self.drep_without_abstain,
            self.spo_yes,
            self.spo_active,
            self.spo_abstain
        )
    }
}

impl AggregatedVotes {
    fn safe_rational(nom: u64, denom: u64) -> Result<RationalNumber> {
        if nom > denom {
            // Also includes variant with denom=0
            bail!("Impossible votes proportion {nom}/{denom}: greater than 1")
        }

        if denom == 0 {
            Ok(RationalNumber::ZERO)
        } else {
            Ok(RationalNumber::new(nom, denom))
        }
    }

    pub fn votes_to_rationals(&self) -> Result<VoteResult<RationalNumber>> {
        let committee_ratio = Self::safe_rational(
            self.committee_yes,
            self.committee_without_abstain, // all non-voted as 'no'
        )?;

        let drep_ratio = Self::safe_rational(self.drep_yes, self.drep_without_abstain)?;

        let spo_ratio = Self::safe_rational(self.spo_yes, self.spo_active - self.spo_abstain)?;

        Ok(VoteResult::<RationalNumber>::new(
            committee_ratio,
            drep_ratio,
            spo_ratio,
        ))
    }

    pub fn get_from_file(
        filename: &Path,
    ) -> Result<HashMap<(u64, GovActionId), (Self, AggregatedVotesOutcome)>> {
        let f = File::open(filename)?;
        let mut reader = BufReader::new(f);

        Self::get_from_reader(&mut reader, filename)
    }

    pub fn get_from_reader<R: Read>(
        reader: &mut R,
        filename: &Path,
    ) -> Result<HashMap<(u64, GovActionId), (Self, AggregatedVotesOutcome)>> {
        let mut reader = csv::ReaderBuilder::new().delimiter(b',').from_reader(reader);
        let mut res = HashMap::new();

        for (n, line) in reader.records().enumerate() {
            let lno = n + 1;
            let split = line?.iter().map(|x| x.to_owned()).collect::<Vec<String>>();
            let [epoch, vhash, idx, cy, cw, dy, dw, py, pw, pa] = &split[0..10] else {
                bail!("Unexpected elements count at line {lno}, file {filename:?}: {split:?}");
            };
            let oc = split.get(10);

            let epoch = epoch.parse::<u64>()?;
            let vhash = hex::decode(vhash)?;
            let idx = idx.parse::<u8>()?;
            let gov_action_id = GovActionId {
                transaction_id: vhash.try_into().map_err(|_| {
                    anyhow::anyhow!("Invalid hash length at line {lno}, file {filename:?}")
                })?,
                action_index: idx,
            };
            let outcome = match oc.map(|x| x.as_str()) {
                None | Some("") => AggregatedVotesOutcome::NoOutcome,
                Some("Expired") => AggregatedVotesOutcome::Expired,
                Some("Ratified") => AggregatedVotesOutcome::Ratified,
                Some(x) => bail!("Unexpected outcome '{x}' at line {lno}, file {filename:?}"),
            };
            let votes = AggregatedVotes {
                committee_yes: cy.parse::<u64>()?,
                committee_without_abstain: cw.parse::<u64>()?,
                drep_yes: dy.parse::<u64>()?,
                drep_without_abstain: dw.parse::<u64>()?,
                spo_yes: py.parse::<u64>()?,
                spo_active: pw.parse::<u64>()?,
                spo_abstain: pa.parse::<u64>()?,
            };
            if let Some(_prev) = res.insert((epoch, gov_action_id.clone()), (votes, outcome)) {
                bail!(
                    "Duplicate entry for epoch {epoch}, gov_action_id {gov_action_id:?} \
                    at line {lno}, file {filename:?}"
                );
            }
        }
        Ok(res)
    }
}

impl Display for AggregatedVotesOutcome {
    fn fmt(&self, res: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(res, "{self:?}")
    }
}

impl AggregatedVotesOutcome {
    pub fn new(accepted: bool, expired: bool) -> Self {
        match (accepted, expired) {
            (true, _) => Self::Ratified,
            (false, true) => Self::Expired,
            (false, false) => Self::NoOutcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        protocol_params::ProtocolVersion, Anchor, CommitteeChange, Constitution,
        HardForkInitiationAction, NewConstitutionAction, StakeAddress, UpdateCommitteeAction,
    };

    fn hard_fork() -> ProposalProcedure {
        ProposalProcedure {
            deposit: 0,
            reward_account: StakeAddress::default(),
            gov_action_id: Default::default(),
            gov_action: GovernanceAction::HardForkInitiation(HardForkInitiationAction {
                previous_action_id: None,
                protocol_version: ProtocolVersion {
                    major: 10,
                    minor: 0,
                },
            }),
            anchor: Anchor {
                data_hash: vec![],
                url: "".to_string(),
            },
        }
    }

    #[test]
    fn test_compare_votes_hardfork() -> Result<()> {
        // Epoch 536 vote, gov_action1pvv5wmjqhwa4u85vu9f4ydmzu2mgt8n7et967ph2urhx53r70xusqnmm525
        // State of epoch
        let voting_state = VotingRegistrationState {
            registered_spos: 21484437730592053,
            registered_dreps: 2834375537437426,
            no_confidence_dreps: 139218917329422,
            abstain_dreps: 1777395232971539,
            committee_size: 7,
        };

        let vc = VoteResult::<VoteCount> {
            committee: VoteCount {
                yes: 7,
                no: 0,
                abstain: 0,
            },
            drep: VoteCount::zero(),
            pool: VoteCount {
                yes: 13444887496098977,
                no: 60273882920902,
                abstain: 797759099341505,
            },
        };

        let th = VoteResult::new(
            RationalNumber::new(2, 3),
            RationalNumber::new(3, 5),
            RationalNumber::new(51, 100),
        );

        let hard_fork = hard_fork();
        let vr = voting_state.aggregate_votes(&hard_fork, true, &vc)?.votes_to_rationals()?;
        println!("Rational votes: {vr:?}");
        println!("Thresholds: {:?}", th);
        assert!(voting_state.compare_votes(true, &vr, &th)?);

        Ok(())
    }

    #[test]
    fn test_compare_votes_constitution() -> Result<()> {
        // Epoch 541 vote, gov_action133jnaewfsq8x6v08ndd87l2yqryp63r30t2dkceacxx5cply5n7sqzlcyqf
        let voting_state = VotingRegistrationState {
            registered_spos: 21519946112812787,
            registered_dreps: 3571497541619844,
            no_confidence_dreps: 156222555815772,
            abstain_dreps: 4948821694344546,
            committee_size: 7,
        };

        let constitution = ProposalProcedure {
            deposit: 0,
            reward_account: StakeAddress::default(),
            gov_action_id: Default::default(),
            gov_action: GovernanceAction::NewConstitution(NewConstitutionAction {
                previous_action_id: None,
                new_constitution: Constitution {
                    anchor: Anchor {
                        data_hash: vec![],
                        url: "".to_string(),
                    },
                    guardrail_script: None,
                },
            }),
            anchor: Anchor {
                data_hash: vec![],
                url: "".to_string(),
            },
        };

        // votes c7/0/0:d2993595142272357/78507823799974/164374042176526:s0/0/0,
        // thresholds c2/3:d3/4:s0, prevous_ok true, voted true, result true
        let vc = VoteResult::<VoteCount> {
            committee: VoteCount {
                yes: 7,
                no: 0,
                abstain: 0,
            },
            drep: VoteCount {
                yes: 2993595142272357,
                no: 78507823799974,
                abstain: 166153144964868,
            },
            pool: VoteCount::zero(),
        };

        let th = VoteResult::<RationalNumber> {
            committee: RationalNumber::new(2, 3),
            drep: RationalNumber::new(3, 4),
            pool: RationalNumber::ZERO,
        };

        let vr = voting_state.aggregate_votes(&constitution, false, &vc)?.votes_to_rationals()?;
        println!("Rational votes: {vr:?}");
        println!("Thresholds: {:?}", th);
        assert!(voting_state.compare_votes(false, &vr, &th)?);

        Ok(())
    }

    #[ignore]
    #[test]
    fn test_compare_votes_committee_update() -> Result<()> {
        // SPO 580:               yesStake=9028434771226062;
        //               totalActiveStake=21887305805085033;
        //               abstainStake    = 7137312985784179
        // relative = yesStake / (totalActiveStake - abstainStake) =
        //            9028434771226062 / (21887305805085033 - 7137312985784179) = 0.612...
        let voting_state = VotingRegistrationState {
            registered_spos: 21887305805085033, // totalActiveStake
            registered_dreps: 5469901625021595 + 4314868062161205 + 39484180309884,
            no_confidence_dreps: 174036425217925,
            abstain_dreps: 7688271305871977, //4948821694344546,
            committee_size: 7,
        };

        let committee_update = ProposalProcedure {
            deposit: 0,
            reward_account: StakeAddress::default(),
            gov_action_id: Default::default(),
            gov_action: GovernanceAction::UpdateCommittee(UpdateCommitteeAction {
                previous_action_id: None,
                data: CommitteeChange {
                    removed_committee_members: Default::default(),
                    new_committee_members: Default::default(),
                    terms: Default::default(),
                },
            }),
            anchor: Anchor {
                data_hash: vec![],
                url: "".to_string(),
            },
        };

        // Proposal gov_action1g7sw0f8e8qa34lppj2erksvzf4j6e9udwaq6efslc8apdqeazygsq2spyyt:
        // new epoch 580,
        // votes cy0/n0/a0:dy4276903074326165/n39020632491545/a117034594321273
        //         :sy7990777286158137/n50066649467555/a7009748575787769,
        //          thresholds c0:d67/100:s51/100, prevous_ok true, voted true, result true
        // Voting DRep epoch=EpochNo 579 action=GovActionId {gaidTxId = TxId {unTxId =
        // SafeHash "47a0e7a4f9383b1afc2192b23b41824d65ac978d7741aca61fc1fa16833d1111"},
        // gaidGovActionIx = GovActionIx {unGovActionIx = 0}};
        // yesStake=4276903074326165, totalExclAbstain=5469901625021595,
        // [(drep,stake,(y,n,a,nv,df,ig))]=[(DRepAlwaysNoConfidence,174036425217925,
        // (0,174036425217925,0,0,1,0)),
        // (DRepAlwaysAbstain,7688271305871977,(0,0,7688271305871977,0,1,0)),
        // Voting SPO epoch=EpochNo 579, action=GovActionId {gaidTxId = TxId {unTxId = SafeHash
        // "47a0e7a4f9383b1afc2192b23b41824d65ac978d7741aca61fc1fa16833d1111"},
        // gaidGovActionIx = GovActionIx {unGovActionIx = 0}};
        // yesStake=7990877286158137; totalActiveStake=21887250568893730, abstainStake=7009748575787769;

        // votes cy0/n0/a0:dy4499101798849121/n41139783152453/a120778537433687
        //         :sy9523082152104886/n46016280922393/a26037085361436
        //       cy0/n0/a0:dy4314868062161205/n39484180309884/a117047215529172
        //         :sy9028234771226062/n43709503763147/a24832116298377
        // thresholds c0:d67/100:s51/100, prevous_ok true, voted true, result true
        let vc = VoteResult::<VoteCount> {
            committee: VoteCount {
                yes: 0,
                no: 0,
                abstain: 0,
            },
            drep: VoteCount {
                yes: 4314868062161205,
                no: 39484180309884,
                abstain: 117047215529172,
            },
            pool: VoteCount {
                yes: 9028234771226062,
                no: 43709503763147,
                abstain: 24832116298377,
            },
        };

        let th = VoteResult::<RationalNumber> {
            committee: RationalNumber::ZERO,
            drep: RationalNumber::new(67, 100),
            pool: RationalNumber::new(51, 100),
        };

        let vr =
            voting_state.aggregate_votes(&committee_update, false, &vc)?.votes_to_rationals()?;
        println!("Rational votes: {:?}", vr);
        println!("Thresholds: {:?}", th);
        assert!(voting_state.compare_votes(false, &vr, &th)?);

        Ok(())
    }

    #[test]
    fn zero_votes() -> Result<()> {
        let voting_state = VotingRegistrationState {
            registered_spos: 0,
            registered_dreps: 0,
            no_confidence_dreps: 0,
            abstain_dreps: 0,
            committee_size: 7,
        };

        let votes = VoteResult::<VoteCount> {
            committee: VoteCount::zero(),
            drep: VoteCount::zero(),
            pool: VoteCount {
                yes: 0,
                no: 0,
                abstain: 0,
            },
        };

        let res =
            voting_state.aggregate_votes(&hard_fork(), false, &votes)?.votes_to_rationals()?;
        println!(
            "{:?}, {}",
            res.committee,
            res.committee >= RationalNumber::ONE
        );
        println!("{:?}, {}", res.drep, res.drep >= RationalNumber::ONE);
        println!("{:?}, {}", res.pool, res.pool >= RationalNumber::ONE);

        let votes = VoteResult::<VoteCount> {
            committee: VoteCount::zero(),
            drep: VoteCount::zero(),
            pool: VoteCount {
                yes: 1,
                no: 0,
                abstain: 0,
            },
        };

        if let Ok(res) =
            voting_state.aggregate_votes(&hard_fork(), false, &votes)?.votes_to_rationals()
        {
            bail!("Must return error: found Ok({res:?})");
        }

        Ok(())
    }

    #[test]
    fn test_aggregated_votes_from_file() -> Result<()> {
        //        -> Result<HashMap<(u64, GovActionId), (Self, AggregatedVotesOutcome)>>
        let votes = AggregatedVotes::get_from_reader(
            &mut "\"epoch\",\"tx\",\"tx-idx\",\"committee-yes\",\"committee-excl-abstain\",\
                \"DRep-yes\",\"DRep-excl-abstain\",\"SPO-yes\",\"SPO-active\",\"SPO-abstain\",\"outcome\"\n\
                561,\"7d9fc9fe4cee64fb34e57783378ac869a85c78d6fbcd4078ed131ab6fa3c7db6\",0,0,0,\
                2349965987191895,5355678135055138,629153824163943,22056240205171748,7538000074152046,\"Expired\"\n\
                574,\"8ad3d454f3496a35cb0d07b0fd32f687f66338b7d60e787fc0a22939e5d8833e\",0,0,0,\
                3976814149513934,5397744536801712,0,21899652138158468,9169378749774697,\"Ratified\"\n\
                574,\"8ad3d454f3496a35cb0d07b0fd32f687f66338b7d60e787fc0a22939e5d8833e\",1,0,0,\
                3759538115826368,5085339952867027,0,21899652138158468,9169378749774697,\"Ratified\""
                .as_bytes(),
            Path::new("")
        )?;

        let v561 = votes
            .get(&(
                561,
                GovActionId::from_bech32(
                    "gov_action10k0unljvaej0kd89w7pn0zkgdx59c7xkl0x5q78dzvdtd73u0kmqq5xl5y5",
                )?,
            ))
            .unwrap();

        assert_eq!(v561.0.drep_yes, 2349965987191895);
        assert_eq!(v561.1, AggregatedVotesOutcome::Expired);

        let v574_1 = votes
            .get(&(
                574,
                GovActionId::from_bech32(
                    "gov_action13tfag48nf94rtjcdq7c06vhkslmxxw9h6c88sl7q5g5nnewcsvlqz6d98zp",
                )?,
            ))
            .unwrap();

        assert_eq!(v574_1.0.drep_yes, 3759538115826368);
        assert_eq!(v574_1.1, AggregatedVotesOutcome::Ratified);

        Ok(())
    }
}
