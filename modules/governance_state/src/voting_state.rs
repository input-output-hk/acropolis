use acropolis_common::{
    GovernanceAction, ConwayParams, VotesCount,
    ProtocolParamType, ProposalProcedure, ProtocolParamUpdate,
    rational_number::RationalNumber
};
use std::{cmp::max, fmt};
use anyhow::Result;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct VotingRegistrationState {
    total_spos: u64,
    registered_spos: u64,
    registered_dreps: u64,
    committee_size: u64,
}

impl fmt::Display for VotingRegistrationState {
    fn fmt(&self, res: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(res, "spos total {}/reg. {}, dreps {},  committee {}",
            self.total_spos, self.registered_spos, self.registered_dreps, self.committee_size
        )
    }
}

impl VotingRegistrationState {
    // At least one vote in each category is enough.
    #[allow(dead_code)]
    pub fn fake() -> Self {
        Self {
            total_spos: 1,
            registered_spos: 1,
            registered_dreps: 1,
            committee_size: 1,
        }
    }


    pub fn new(
        total_spos: u64, registered_spos: u64, registered_dreps: u64, committee_size: u64
    ) -> Self {
        Self { total_spos, registered_spos, registered_dreps, committee_size }
    }

    fn proportional_count_drep_comm(
        &self,
        drep: &RationalNumber,
        comm: &RationalNumber,
    ) -> Result<(u64, u64)> {
        let d = drep.proportion_of(self.registered_dreps)?.round_up();
        let c = comm.proportion_of(self.committee_size)?.round_up();
        Ok((d, c))
    }

    fn proportional_count(
        &self,
        pool: &RationalNumber,
        drep: &RationalNumber,
        comm: &RationalNumber,
    ) -> Result<VotesCount> {
        let mut votes = VotesCount::zero();
        votes.pool = pool.proportion_of(self.registered_spos)?.round_up();
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
        votes.pool = pool.proportion_of(self.total_spos)?.round_up();
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
    pub fn get_action_thresholds(
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
}
