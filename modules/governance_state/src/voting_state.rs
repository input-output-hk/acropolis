use acropolis_common::{
    protocol_params::ConwayParams, rational_number::RationalNumber, GovernanceAction,
    ProposalProcedure, ProtocolParamType, ProtocolParamUpdate, VoteCount, VoteResult,
};
use anyhow::{bail, Result};
use std::{cmp::max, fmt};
use tracing::error;

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

    fn safe_rational(nom: u64, denom: u64) -> Result<RationalNumber> {
        if nom > denom {
            bail!("Impossible votes proportion {nom}/{denom}: greater than 1")
        }

        if denom == 0 {
            Ok(RationalNumber::ZERO)
        } else {
            Ok(RationalNumber::new(nom, denom))
        }
    }

    fn votes_to_rationals(
        &self,
        proc: &ProposalProcedure,
        _bootstrap: bool,
        votes: &VoteResult<VoteCount>,
    ) -> Result<VoteResult<RationalNumber>> {
        // Only 'info', 'hardfork' and 'parameter change' actions are allowed in bootstrap period
        // committee vote thresholds
        if votes.committee.total() > self.committee_size {
            bail!(
                "Committee vote count {} > committee size {}",
                votes.committee.total(),
                self.committee_size
            );
        }

        let committee_ratio = Self::safe_rational(
            votes.committee.yes,
            self.committee_size - votes.committee.abstain, // all non-voted as 'no'
        )?;

        // DRep vote thresholds
        let total_dreps = self.registered_dreps + self.abstain_dreps + self.no_confidence_dreps;
        let mut non_voted = VoteCount::zero();
        non_voted.abstain = self.abstain_dreps;

        // Any DRep, which did not vote and has no default vote, is considered voting 'No'
        non_voted.no = self.registered_dreps - votes.drep.total();

        if let GovernanceAction::NoConfidence(_) = proc.gov_action {
            non_voted.yes += self.no_confidence_dreps
        } else {
            non_voted.no += self.no_confidence_dreps;
        };

        if non_voted.total() + votes.drep.total() != total_dreps {
            bail!("Total votes (including votes from non-voted) != total dreps stake");
        }

        let total_yes = votes.drep.yes + non_voted.yes;
        let total_no = votes.drep.no + non_voted.no;
        let drep_ratio = Self::safe_rational(total_yes, total_yes + total_no)?;

        // SPO vote thresholds
        // TODO: always abstain, no confidence spo's (present in Haskell code, for bootstrap)
        let spo_ratio =
            RationalNumber::new(votes.pool.yes, self.registered_spos - votes.pool.abstain);

        Ok(VoteResult::<RationalNumber>::new(
            committee_ratio,
            drep_ratio,
            spo_ratio,
        ))
    }

    pub fn compare_votes(
        &self,
        proc: &ProposalProcedure,
        bootstrap: bool,
        votes: &VoteResult<VoteCount>,
        threshold: &VoteResult<RationalNumber>,
    ) -> Result<bool> {
        if bootstrap {
            match &proc.gov_action {
                GovernanceAction::TreasuryWithdrawals(_)
                | GovernanceAction::NewConstitution(_)
                | GovernanceAction::UpdateCommittee(_)
                | GovernanceAction::NoConfidence(_) => {
                    error!(
                        "Action {} ({}) is not possible in bootstrap Conway (Chang) era.",
                        proc.gov_action_id,
                        &proc.gov_action.get_action_name()
                    );
                    return Ok(false);
                }

                _ => (),
            }
        }

        let rational_votes = self.votes_to_rationals(proc, bootstrap, votes)?;

        Ok(rational_votes.committee >= threshold.committee
            && (bootstrap || rational_votes.drep >= threshold.drep) // dreps ignored at bootstrap
            && rational_votes.pool >= threshold.pool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        protocol_params::ProtocolVersion, Anchor, Constitution, HardForkInitiationAction,
        NewConstitutionAction, StakeAddress,
    };

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

        let hard_fork = ProposalProcedure {
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
        };

        let vr = VoteResult::<VoteCount> {
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

        println!(
            "Rational votes: {:?}",
            voting_state.votes_to_rationals(&hard_fork, true, &vr)
        );
        println!("Thresholds: {:?}", th);

        assert!(voting_state.compare_votes(&hard_fork, true, &vr, &th)?);

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
        let vr = VoteResult::<VoteCount> {
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

        println!(
            "Rational votes: {:?}",
            voting_state.votes_to_rationals(&constitution, false, &vr)
        );
        println!("Thresholds: {:?}", th);

        assert!(voting_state.compare_votes(&constitution, false, &vr, &th)?);

        Ok(())
    }
}
