use acropolis_common::{
    Credential, DRepChoice, GovActionId, GovernanceAction, ProposalProcedure, Vote, Voter,
    VotingProcedures, Withdrawal,
};
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use super::to_plutus_data::*;
use acropolis_common::validation::ScriptContextError;

// ============================================================================
// Withdrawals
// ============================================================================

pub fn encode_withdrawals<'a>(
    withdrawals: &[Withdrawal],
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match version {
        // V1: List of 2-tuples [Constr(0, [StakingHash(cred), amount])]
        PlutusVersion::V1 => {
            let tuples: Vec<_> = withdrawals
                .iter()
                .map(|w| {
                    let cred = w.address.credential.to_plutus_data(arena, version)?;
                    let key = constr(arena, 0, vec![cred]); // StakingHash wrapper
                    let val = integer(arena, w.value as i128);
                    Ok(constr(arena, 0, vec![key, val]))
                })
                .collect::<Result<_, ScriptContextError>>()?;
            Ok(list(arena, tuples))
        }
        // V2: Map of (StakingHash(cred), amount)
        PlutusVersion::V2 => {
            let pairs: Vec<_> = withdrawals
                .iter()
                .map(|w| {
                    let cred = w.address.credential.to_plutus_data(arena, version)?;
                    let key = constr(arena, 0, vec![cred]);
                    let val = integer(arena, w.value as i128);
                    Ok((key, val))
                })
                .collect::<Result<_, ScriptContextError>>()?;
            Ok(map(arena, pairs))
        }
        // V3: Map of (credential, amount) - bare credential, no StakingHash wrapper
        PlutusVersion::V3 => {
            let pairs: Vec<_> = withdrawals
                .iter()
                .map(|w| {
                    let key = w.address.credential.to_plutus_data(arena, version)?;
                    let val = integer(arena, w.value as i128);
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

pub fn encode_drep_choice<'a>(
    drep: &DRepChoice,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match drep {
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

// ============================================================================
// Voter (V3)
// ============================================================================

pub fn encode_voter<'a>(
    voter: &Voter,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match voter {
        Voter::ConstitutionalCommitteeKey(hash) => {
            let cred = Credential::AddrKeyHash(hash.into_inner()).to_plutus_data(arena, version)?;
            Ok(constr(arena, 0, vec![cred]))
        }
        Voter::ConstitutionalCommitteeScript(hash) => {
            let cred = Credential::ScriptHash(hash.into_inner()).to_plutus_data(arena, version)?;
            Ok(constr(arena, 0, vec![cred]))
        }
        Voter::DRepKey(hash) => {
            let cred = Credential::AddrKeyHash(hash.into_inner()).to_plutus_data(arena, version)?;
            Ok(constr(arena, 1, vec![cred]))
        }
        Voter::DRepScript(hash) => {
            let cred = Credential::ScriptHash(hash.into_inner()).to_plutus_data(arena, version)?;
            Ok(constr(arena, 1, vec![cred]))
        }
        Voter::StakePoolKey(pool_id) => {
            let pid = pool_id.to_plutus_data(arena, version)?;
            Ok(constr(arena, 2, vec![pid]))
        }
    }
}

// ============================================================================
// Vote (V3)
// ============================================================================

pub fn encode_vote<'a>(
    vote: &Vote,
    arena: &'a Arena,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match vote {
        Vote::No => Ok(constr(arena, 0, vec![])),
        Vote::Yes => Ok(constr(arena, 1, vec![])),
        Vote::Abstain => Ok(constr(arena, 2, vec![])),
    }
}

// ============================================================================
// GovActionId
// ============================================================================

pub fn encode_gov_action_id<'a>(
    gaid: &GovActionId,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let tx_id = constr(
        arena,
        0,
        vec![gaid.transaction_id.to_plutus_data(arena, version)?],
    );
    let idx = integer(arena, gaid.action_index as i128);
    Ok(constr(arena, 0, vec![tx_id, idx]))
}

pub fn encode_maybe_gov_action_id<'a>(
    id: &Option<GovActionId>,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match id {
        Some(gaid) => {
            let encoded = encode_gov_action_id(gaid, arena, version)?;
            Ok(constr(arena, 0, vec![encoded]))
        }
        None => Ok(constr(arena, 1, vec![])),
    }
}

// ============================================================================
// VotingProcedures (V3)
// ============================================================================

pub fn encode_voting_procedures<'a>(
    vp: &VotingProcedures,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let pairs: Vec<_> = vp
        .votes
        .iter()
        .map(|(voter, single_votes)| {
            let voter_pd = encode_voter(voter, arena, version)?;
            let inner: Vec<_> = single_votes
                .voting_procedures
                .iter()
                .map(|(gaid, procedure)| {
                    let gaid_pd = encode_gov_action_id(gaid, arena, version)?;
                    let vote_pd = encode_vote(&procedure.vote, arena)?;
                    Ok((gaid_pd, vote_pd))
                })
                .collect::<Result<_, ScriptContextError>>()?;
            Ok((voter_pd, map(arena, inner)))
        })
        .collect::<Result<_, ScriptContextError>>()?;
    Ok(map(arena, pairs))
}

// ============================================================================
// GovernanceAction (V3)
// ============================================================================

pub fn encode_governance_action<'a>(
    action: &GovernanceAction,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match action {
        GovernanceAction::ParameterChange(pca) => {
            let prev = encode_maybe_gov_action_id(&pca.previous_action_id, arena, version)?;
            let params = integer(arena, 0); // placeholder for ChangedParameters
            let script = match &pca.script_hash {
                Some(h) => constr(arena, 0, vec![h.to_plutus_data(arena, version)?]),
                None => constr(arena, 1, vec![]),
            };
            Ok(constr(arena, 0, vec![prev, params, script]))
        }
        GovernanceAction::HardForkInitiation(hfi) => {
            let prev = encode_maybe_gov_action_id(&hfi.previous_action_id, arena, version)?;
            let pv = constr(
                arena,
                0,
                vec![
                    integer(arena, hfi.protocol_version.major as i128),
                    integer(arena, hfi.protocol_version.minor as i128),
                ],
            );
            Ok(constr(arena, 1, vec![prev, pv]))
        }
        GovernanceAction::TreasuryWithdrawals(tw) => {
            let wdrl_pairs: Vec<_> = tw
                .rewards
                .iter()
                .map(|(addr_bytes, amount)| {
                    let k = bytes(arena, addr_bytes);
                    let v = integer(arena, *amount as i128);
                    (k, v)
                })
                .collect();
            let wdrl_map = map(arena, wdrl_pairs);
            let script = match &tw.script_hash {
                Some(h) => constr(arena, 0, vec![h.to_plutus_data(arena, version)?]),
                None => constr(arena, 1, vec![]),
            };
            Ok(constr(arena, 2, vec![wdrl_map, script]))
        }
        GovernanceAction::NoConfidence(prev) => {
            let prev_pd = encode_maybe_gov_action_id(prev, arena, version)?;
            Ok(constr(arena, 3, vec![prev_pd]))
        }
        GovernanceAction::UpdateCommittee(uc) => {
            let prev = encode_maybe_gov_action_id(&uc.previous_action_id, arena, version)?;
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
            let prev = encode_maybe_gov_action_id(&nc.previous_action_id, arena, version)?;
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

// ============================================================================
// ProposalProcedure (V3)
// ============================================================================

pub fn encode_proposal_procedure<'a>(
    proposal: &ProposalProcedure,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let deposit = integer(arena, proposal.deposit as i128);
    let reward_acct = proposal.reward_account.credential.to_plutus_data(arena, version)?;
    let action = encode_governance_action(&proposal.gov_action, arena, version)?;
    Ok(constr(arena, 0, vec![deposit, reward_acct, action]))
}
