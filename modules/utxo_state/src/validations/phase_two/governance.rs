use acropolis_common::{
    validation::ScriptContextError, Credential, DRepChoice, GovActionId, GovernanceAction,
    ProposalProcedure, StakeAddress, Vote, Voter, VotingProcedures, Withdrawal,
};
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use crate::validations::phase_two::utils::cmp_withdrawal;

use super::to_plutus_data::*;

pub fn encode_withdrawals<'a>(
    withdrawals: &[Withdrawal],
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let sorted_withdrawals = {
        let mut w = withdrawals.to_vec();
        w.sort_by(|a, b| cmp_withdrawal(a, b, version));
        w
    };
    match version {
        // V1: List of 2-tuples — `[(StakingCredential, Integer)]` uses
        // standard list-of-tuples ToData, producing List [Constr(0, [key, val])...]
        PlutusVersion::V1 => {
            let tuples: Vec<_> = sorted_withdrawals
                .iter()
                .map(|w| {
                    let cred = w.address.credential.to_plutus_data(arena, version)?;
                    let key = constr(arena, 0, vec![cred]); // StakingHash wrapper
                    let val = w.value.to_plutus_data(arena, version)?;
                    Ok(constr(arena, 0, vec![key, val]))
                })
                .collect::<Result<_, ScriptContextError>>()?;
            Ok(list(arena, tuples))
        }
        // V2: Map of (StakingHash(cred), amount)
        PlutusVersion::V2 => {
            let pairs: Vec<_> = sorted_withdrawals
                .iter()
                .map(|w| {
                    let cred = w.address.credential.to_plutus_data(arena, version)?;
                    let key = constr(arena, 0, vec![cred]); // StakingHash wrapper
                    let val = w.value.to_plutus_data(arena, version)?;
                    Ok((key, val))
                })
                .collect::<Result<_, ScriptContextError>>()?;
            Ok(map(arena, pairs))
        }
        // V3: Map of (credential, amount) - bare credential, no StakingHash wrapper
        PlutusVersion::V3 => {
            let pairs: Vec<_> = sorted_withdrawals
                .iter()
                .map(|w| {
                    let key = w.address.credential.to_plutus_data(arena, version)?;
                    let val = w.value.to_plutus_data(arena, version)?;
                    Ok((key, val))
                })
                .collect::<Result<_, ScriptContextError>>()?;
            Ok(map(arena, pairs))
        }
    }
}

// ============================================================================
// DRepChoice (V3)
// ============================================================================

impl ToPlutusData for DRepChoice {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match self {
            DRepChoice::Key(hash) => {
                let cred = Credential::AddrKeyHash(*hash).to_plutus_data(arena, version)?;
                Ok(constr(arena, 0, vec![cred]))
            }
            DRepChoice::Script(hash) => {
                let cred = Credential::ScriptHash(*hash).to_plutus_data(arena, version)?;
                Ok(constr(arena, 0, vec![cred]))
            }
            DRepChoice::Abstain => Ok(constr(arena, 1, vec![])),
            DRepChoice::NoConfidence => Ok(constr(arena, 2, vec![])),
        }
    }
}

// ============================================================================
// Voter (V3)
// ============================================================================

impl ToPlutusData for Voter {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match self {
            Voter::ConstitutionalCommitteeKey(hash) => {
                let cred =
                    Credential::AddrKeyHash(hash.into_inner()).to_plutus_data(arena, version)?;
                Ok(constr(arena, 0, vec![cred]))
            }
            Voter::ConstitutionalCommitteeScript(hash) => {
                let cred =
                    Credential::ScriptHash(hash.into_inner()).to_plutus_data(arena, version)?;
                Ok(constr(arena, 0, vec![cred]))
            }
            Voter::DRepKey(hash) => {
                let cred =
                    Credential::AddrKeyHash(hash.into_inner()).to_plutus_data(arena, version)?;
                Ok(constr(arena, 1, vec![cred]))
            }
            Voter::DRepScript(hash) => {
                let cred =
                    Credential::ScriptHash(hash.into_inner()).to_plutus_data(arena, version)?;
                Ok(constr(arena, 1, vec![cred]))
            }
            Voter::StakePoolKey(pool_id) => {
                let pid = pool_id.to_plutus_data(arena, version)?;
                Ok(constr(arena, 2, vec![pid]))
            }
        }
    }
}

// ============================================================================
// Vote (V3)
// ============================================================================

impl ToPlutusData for Vote {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        _version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match self {
            Vote::No => Ok(constr(arena, 0, vec![])),
            Vote::Yes => Ok(constr(arena, 1, vec![])),
            Vote::Abstain => Ok(constr(arena, 2, vec![])),
        }
    }
}

// ============================================================================
// GovActionId
// ============================================================================

impl ToPlutusData for GovActionId {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let tx_id = self.transaction_id.to_plutus_data(arena, version)?;
        let idx = self.action_index.to_plutus_data(arena, version)?;
        Ok(constr(arena, 0, vec![tx_id, idx]))
    }
}

// ============================================================================
// VotingProcedures (V3)
// ============================================================================

impl ToPlutusData for VotingProcedures {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let sorted_votes = self.sorted_votes();
        let pairs: Vec<_> = sorted_votes
            .iter()
            .map(|(voter, single_votes)| {
                let voter_pd = voter.to_plutus_data(arena, version)?;
                let inner: Vec<_> = single_votes
                    .iter()
                    .map(|(gaid, procedure)| {
                        let gaid_pd = gaid.to_plutus_data(arena, version)?;
                        let vote_pd = procedure.vote.to_plutus_data(arena, version)?;
                        Ok((gaid_pd, vote_pd))
                    })
                    .collect::<Result<_, ScriptContextError>>()?;
                Ok((voter_pd, map(arena, inner)))
            })
            .collect::<Result<_, ScriptContextError>>()?;
        Ok(map(arena, pairs))
    }
}

// ============================================================================
// GovernanceAction (V3)
// ============================================================================

impl ToPlutusData for GovernanceAction {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match self {
            GovernanceAction::ParameterChange(pca) => {
                let prev = pca.previous_action_id.to_plutus_data(arena, version)?;
                // TODO:
                // implement a proper encoding for the parameters instead of just using an integer placeholder
                let params = integer(arena, 0);
                let script = match &pca.script_hash {
                    Some(h) => constr(arena, 0, vec![h.to_plutus_data(arena, version)?]),
                    None => constr(arena, 1, vec![]),
                };
                Ok(constr(arena, 0, vec![prev, params, script]))
            }
            GovernanceAction::HardForkInitiation(hfi) => {
                let prev = hfi.previous_action_id.to_plutus_data(arena, version)?;
                let pv = hfi.protocol_version.to_plutus_data(arena, version)?;
                Ok(constr(arena, 1, vec![prev, pv]))
            }
            GovernanceAction::TreasuryWithdrawals(tw) => {
                let wdrl_pairs: Vec<_> = tw
                    .rewards
                    .iter()
                    .map(|(addr_bytes, amount)| {
                        let reward_acc = StakeAddress::from_binary(addr_bytes)
                            .expect("Invalid stake address in Treasury Withdrawals");
                        let k = reward_acc.to_plutus_data(arena, version)?;
                        let v = amount.to_plutus_data(arena, version)?;
                        Ok((k, v))
                    })
                    .collect::<Result<_, ScriptContextError>>()?;
                let wdrl_map = map(arena, wdrl_pairs);
                let script = tw.script_hash.to_plutus_data(arena, version)?;
                Ok(constr(arena, 2, vec![wdrl_map, script]))
            }
            GovernanceAction::NoConfidence(prev) => {
                let prev_pd = prev.to_plutus_data(arena, version)?;
                Ok(constr(arena, 3, vec![prev_pd]))
            }
            GovernanceAction::UpdateCommittee(uc) => {
                let prev = uc.previous_action_id.to_plutus_data(arena, version)?;
                let removed: Vec<_> = uc
                    .data
                    .removed_committee_members
                    .iter()
                    .map(|cred| cred.to_plutus_data(arena, version))
                    .collect::<Result<_, _>>()?;
                let removed_list = list(arena, removed);
                let new_pairs: Vec<_> = uc
                    .data
                    .new_committee_members
                    .iter()
                    .map(|(cred, epoch)| {
                        let c = cred.to_plutus_data(arena, version)?;
                        let e = integer(arena, *epoch as i128);
                        Ok((c, e))
                    })
                    .collect::<Result<_, ScriptContextError>>()?;
                let new_map = map(arena, new_pairs);
                let quorum = constr(
                    arena,
                    0,
                    vec![
                        integer(arena, *uc.data.terms.numer() as i128),
                        integer(arena, *uc.data.terms.denom() as i128),
                    ],
                );
                Ok(constr(arena, 4, vec![prev, removed_list, new_map, quorum]))
            }
            GovernanceAction::NewConstitution(nc) => {
                let prev = nc.previous_action_id.to_plutus_data(arena, version)?;
                let guardrail = match &nc.new_constitution.guardrail_script {
                    Some(h) => constr(arena, 0, vec![h.to_plutus_data(arena, version)?]),
                    None => constr(arena, 1, vec![]),
                };
                let constitution = constr(arena, 0, vec![guardrail]);
                Ok(constr(arena, 5, vec![prev, constitution]))
            }
            GovernanceAction::Information => Ok(constr(arena, 6, vec![])),
        }
    }
}

// ============================================================================
// ProposalProcedure (V3)
// ============================================================================

impl ToPlutusData for ProposalProcedure {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let deposit = self.deposit.to_plutus_data(arena, version)?;
        let reward_acct = self.reward_account.credential.to_plutus_data(arena, version)?;
        let action = self.gov_action.to_plutus_data(arena, version)?;
        Ok(constr(arena, 0, vec![deposit, reward_acct, action]))
    }
}
