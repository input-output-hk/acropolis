// SPDX-License-Identifier: Apache-2.0
// Copyright 2025, Acropolis team.

//! Governance state decoding from Cardano snapshots.
//!
//! This module provides CBOR decoding for the governance state (`gov_state`) portion
//! of the NewEpochState snapshot. The governance state contains:
//!
//! - Active governance proposals with their votes
//! - Committee state (hot/cold credentials, authorizations)
//! - Constitution (anchor and guardrail script)
//! - Protocol parameters (current, previous, future)
//! - DRep pulsing state (votes and ratification status)
//!
//! The main entry point is `parse_gov_state` which extracts all governance data
//! needed to bootstrap the `ConwayVoting` state.

use anyhow::{anyhow, Context, Result};
use minicbor::data::Type;
use minicbor::Decoder;
use std::collections::HashMap;
use tracing::info;

use crate::hash::Hash;
use crate::protocol_params::ProtocolVersion;
use crate::snapshot::protocol_parameters::{CurrentParams, FutureParams};
use crate::snapshot::streaming_snapshot::Anchor;
use crate::{
    CommitteeChange, CommitteeCredential, Constitution, Credential, GovActionId, GovernanceAction,
    HardForkInitiationAction, Lovelace, NewConstitutionAction, ParameterChangeAction, PoolId,
    ProposalProcedure, ProtocolParamUpdate, RewardParams, StakeAddress, TreasuryWithdrawalsAction,
    TxHash, UpdateCommitteeAction, Vote, Voter, VotingProcedure,
};

// Re-export types needed by consumers
pub use crate::rational_number::RationalNumber;
pub use crate::Committee;

// ============================================================================
// Type Aliases for Complex Types
// ============================================================================

/// Votes map: action_id -> voter -> voting_procedure
pub type VotesMap = HashMap<GovActionId, HashMap<Voter, VotingProcedure>>;

/// Result of parsing DRep pulsing state
/// (votes, enacted_actions, expired_action_ids, enacted_withdrawals)
pub type DRepPulsingResult = (
    VotesMap,
    Vec<GovActionState>,
    Vec<GovActionId>,
    HashMap<Credential, Lovelace>,
);

/// Data needed for ConwayVoting bootstrap: (proposals, votes)
pub type ConwayVotingData = (Vec<(u64, ProposalProcedure)>, VotesMap);

// ============================================================================
// Gov State Container Types
// ============================================================================

/// Decoded governance state from the snapshot
#[derive(Debug, Clone)]
pub struct GovernanceState {
    /// Epoch the snapshot was taken from
    pub epoch: u64,
    /// Active proposals with their voting state
    pub proposals: Vec<GovActionState>,
    /// Previous governance action IDs by purpose
    pub proposal_roots: GovRelation,
    /// Current constitutional committee (if any)
    pub committee: Option<Committee>,
    /// Current constitution
    pub constitution: Constitution,
    /// Current reward parameters for E-1 reward calculation
    pub current_reward_params: RewardParams,
    /// Previous reward parameters for E-2 reward calculation
    pub previous_reward_params: RewardParams,
    /// Protocol parameters which will be valid after epoch transition
    pub protocol_params: ProtocolParamUpdate,
    /// Votes cast on proposals (from drep_pulsing_state)
    pub votes: HashMap<GovActionId, HashMap<Voter, VotingProcedure>>,
    /// Actions that have been ratified but not yet enacted
    pub enacted_actions: Vec<GovActionState>,
    /// Actions that have expired
    pub expired_action_ids: Vec<GovActionId>,
    /// Treasury withdrawals from enact_state (credential -> amount)
    /// These are withdrawals that have been enacted via governance.
    /// Accounts with withdrawals here should NOT receive pulsing rewards
    /// as they've already been credited.
    pub enacted_withdrawals: HashMap<Credential, Lovelace>,
}

/// Gov action state - proposal with votes and timing info
#[derive(Debug, Clone)]
pub struct GovActionState {
    /// Governance action ID
    pub id: GovActionId,
    /// Committee votes
    pub committee_votes: HashMap<CommitteeCredential, Vote>,
    /// DRep votes
    pub drep_votes: HashMap<Credential, Vote>,
    /// Stake pool votes
    pub stake_pool_votes: HashMap<PoolId, Vote>,
    /// The proposal procedure
    pub proposal_procedure: ProposalProcedure,
    /// Epoch when proposed
    pub proposed_in: u64,
    /// Epoch when it expires
    pub expires_after: u64,
}

/// Governance relation - tracks previous action IDs for each governance purpose
#[derive(Debug, Clone, Default)]
pub struct GovRelation {
    /// Previous parameter update action
    pub pparam_update: Option<GovActionId>,
    /// Previous hard fork action
    pub hard_fork: Option<GovActionId>,
    /// Previous committee action
    pub committee: Option<GovActionId>,
    /// Previous constitution action
    pub constitution: Option<GovActionId>,
}

// ============================================================================
// CBOR Decoding Implementation
// ============================================================================

/// Parse governance state from a CBOR decoder positioned at gov_state
///
/// Expected structure:
/// ```text
/// gov_state = [
///   gs_proposals : proposals,
///   gs_committee : strict_maybe<committee>,
///   gs_constitution : constitution,
///   gs_current_pparams : pparams,
///   gs_previous_pparams : pparams,
///   gs_future_pparams : future_pparams,
///   gs_drep_pulsing_state : drep_pulsing_state
/// ]
/// ```
pub fn parse_gov_state(decoder: &mut Decoder, epoch: u64) -> Result<GovernanceState> {
    info!("    Parsing governance state for epoch {epoch}...");

    let gov_state_len = decoder
        .array()
        .context("Failed to parse gov_state array")?
        .ok_or_else(|| anyhow!("gov_state must be a definite-length array"))?;

    if gov_state_len < 7 {
        return Err(anyhow!(
            "gov_state array too short: expected 7 elements, got {gov_state_len}"
        ));
    }

    // Parse proposals [0]
    let (proposals, proposal_roots) =
        parse_proposals(decoder).context("Failed to parse proposals")?;
    info!("      Parsed {} proposals", proposals.len());

    // Parse committee [1] - strict_maybe<committee>
    let committee = parse_strict_maybe_committee(decoder).context("Failed to parse committee")?;
    if committee.is_some() {
        info!("      Parsed committee with members");
    }

    // Parse constitution [2]
    let constitution = parse_constitution(decoder).context("Failed to parse constitution")?;
    info!("      Parsed constitution: {}", constitution.anchor.url);

    // Parse current_pparams [3], previous_pparams [4], future_pparams [5]
    let current_pparams: ProtocolParamUpdate =
        decoder.decode().context("Failed to decode gs_current_pparams")?;
    let current_reward_params = current_pparams.to_reward_params()?;

    let previous_reward_params: RewardParams = decoder
        .decode::<ProtocolParamUpdate>()
        .context("Failed to decode previous pparams")?
        .to_reward_params()?;

    let protocol_params: ProtocolParamUpdate = decoder
        .decode_with::<CurrentParams, FutureParams>(&mut CurrentParams {
            current: &current_pparams,
        })?
        .0;

    // Parse drep_pulsing_state [6]
    let (votes, enacted_actions, expired_action_ids, enacted_withdrawals) =
        parse_drep_pulsing_state(decoder).context("Failed to parse drep_pulsing_state")?;
    info!(
        "      Parsed {} voting records, {} enacted, {} expired, {} withdrawals",
        votes.len(),
        enacted_actions.len(),
        expired_action_ids.len(),
        enacted_withdrawals.len()
    );

    Ok(GovernanceState {
        epoch,
        proposals,
        proposal_roots,
        committee,
        constitution,
        current_reward_params,
        previous_reward_params,
        protocol_params,
        votes,
        enacted_actions,
        expired_action_ids,
        enacted_withdrawals,
    })
}

/// Parse proposals from gov_state
///
/// proposals = [
///   proposals_roots : proposal_roots,
///   proposals_props : [* gov_action_state]
/// ]
fn parse_proposals(decoder: &mut Decoder) -> Result<(Vec<GovActionState>, GovRelation)> {
    decoder.array().context("Failed to parse proposals array")?;

    // Parse proposal_roots (gov_relation)
    let roots = parse_gov_relation(decoder).context("Failed to parse proposal_roots")?;

    // Parse proposals_props - array of gov_action_state
    let props_len = decoder.array().context("Failed to parse proposals_props array")?;

    let mut proposals = Vec::new();

    match props_len {
        Some(len) => {
            for i in 0..len {
                let state = parse_gov_action_state(decoder)
                    .with_context(|| format!("Failed to parse proposal #{i}"))?;
                proposals.push(state);
            }
        }
        None => {
            // Indefinite-length array
            loop {
                match decoder.datatype()? {
                    Type::Break => {
                        decoder.skip()?;
                        break;
                    }
                    _ => {
                        let state =
                            parse_gov_action_state(decoder).context("Failed to parse proposal")?;
                        proposals.push(state);
                    }
                }
            }
        }
    }

    Ok((proposals, roots))
}

/// Parse gov_relation (proposal roots)
///
/// gov_relation = [
///   gov_relation_pparam_update : strict_maybe<gov_purpose_id<pparam_update_purpose>>,
///   gov_relation_hard_fork : strict_maybe<gov_purpose_id<hard_fork_purpose>>,
///   gov_relation_committee : strict_maybe<gov_purpose_id<committee_purpose>>,
///   gov_relation_constitution : strict_maybe<gov_purpose_id<constitution_purpose>>
/// ]
fn parse_gov_relation(decoder: &mut Decoder) -> Result<GovRelation> {
    decoder.array().context("Failed to parse gov_relation array")?;

    let pparam_update = parse_strict_maybe_gov_action_id(decoder)?;
    let hard_fork = parse_strict_maybe_gov_action_id(decoder)?;
    let committee = parse_strict_maybe_gov_action_id(decoder)?;
    let constitution = parse_strict_maybe_gov_action_id(decoder)?;

    Ok(GovRelation {
        pparam_update,
        hard_fork,
        committee,
        constitution,
    })
}

/// Parse strict_maybe<gov_action_id>
fn parse_strict_maybe_gov_action_id(decoder: &mut Decoder) -> Result<Option<GovActionId>> {
    let len = decoder.array().context("Failed to parse strict_maybe array")?;

    match len {
        Some(0) => Ok(None), // Nothing
        Some(1) | None => {
            // Just(value) - parse the gov_action_id
            let id = parse_gov_action_id(decoder)?;
            Ok(Some(id))
        }
        Some(n) => Err(anyhow!("Invalid strict_maybe length: {n}")),
    }
}

/// Parse gov_action_id
///
/// gov_action_id = [tx_id, gov_action_ix]
fn parse_gov_action_id(decoder: &mut Decoder) -> Result<GovActionId> {
    decoder.array().context("Failed to parse gov_action_id array")?;

    let tx_id_bytes = decoder.bytes().context("Failed to parse tx_id")?;
    let tx_hash: TxHash = tx_id_bytes.try_into().map_err(|_| anyhow!("Invalid tx_id length"))?;

    let action_index = decoder.u8().context("Failed to parse gov_action_ix")?;

    Ok(GovActionId {
        transaction_id: tx_hash,
        action_index,
    })
}

/// Parse gov_action_state
///
/// gov_action_state = [
///   gov_as_id : gov_action_id,
///   gov_as_committee_votes : { * credential_hotcommitteerole => vote },
///   gov_as_drep_votes : { * credential_dreprole => vote },
///   gov_as_stake_pool_votes : { * keyhash_stakepool => vote },
///   gov_as_proposal_procedure : proposal_procedure,
///   gov_as_proposed_in : epoch_no,
///   gov_as_expires_after : epoch_no,
/// ]
fn parse_gov_action_state(decoder: &mut Decoder) -> Result<GovActionState> {
    decoder.array().context("Failed to parse gov_action_state array")?;

    // Parse gov_action_id [0]
    let id = parse_gov_action_id(decoder).context("Failed to parse gov_as_id")?;

    // Parse committee_votes [1]
    let committee_votes =
        parse_vote_map_credential(decoder).context("Failed to parse committee_votes")?;

    // Parse drep_votes [2]
    let drep_votes = parse_vote_map_credential(decoder).context("Failed to parse drep_votes")?;

    // Parse stake_pool_votes [3]
    let stake_pool_votes =
        parse_vote_map_pool(decoder).context("Failed to parse stake_pool_votes")?;

    // Parse proposal_procedure [4]
    let proposal_procedure = parse_proposal_procedure(decoder, id.clone())
        .context("Failed to parse proposal_procedure")?;

    // Parse proposed_in [5]
    let proposed_in = decoder.u64().context("Failed to parse proposed_in")?;

    // Parse expires_after [6]
    let expires_after = decoder.u64().context("Failed to parse expires_after")?;

    Ok(GovActionState {
        id,
        committee_votes,
        drep_votes,
        stake_pool_votes,
        proposal_procedure,
        proposed_in,
        expires_after,
    })
}

/// Parse vote map with credential keys
fn parse_vote_map_credential(decoder: &mut Decoder) -> Result<HashMap<Credential, Vote>> {
    let mut votes = HashMap::new();

    let map_len = decoder.map().context("Failed to parse vote map")?;

    match map_len {
        Some(len) => {
            for _ in 0..len {
                let credential = parse_credential(decoder)?;
                let vote = parse_vote(decoder)?;
                votes.insert(credential, vote);
            }
        }
        None => loop {
            match decoder.datatype()? {
                Type::Break => {
                    decoder.skip()?;
                    break;
                }
                _ => {
                    let credential = parse_credential(decoder)?;
                    let vote = parse_vote(decoder)?;
                    votes.insert(credential, vote);
                }
            }
        },
    }

    Ok(votes)
}

/// Parse vote map with pool ID keys
fn parse_vote_map_pool(decoder: &mut Decoder) -> Result<HashMap<PoolId, Vote>> {
    let mut votes = HashMap::new();

    let map_len = decoder.map().context("Failed to parse vote map")?;

    match map_len {
        Some(len) => {
            for _ in 0..len {
                let pool_bytes = decoder.bytes().context("Failed to parse pool id")?;
                let pool_id: PoolId =
                    pool_bytes.try_into().map_err(|_| anyhow!("Invalid pool id length"))?;
                let vote = parse_vote(decoder)?;
                votes.insert(pool_id, vote);
            }
        }
        None => loop {
            match decoder.datatype()? {
                Type::Break => {
                    decoder.skip()?;
                    break;
                }
                _ => {
                    let pool_bytes = decoder.bytes()?;
                    let pool_id: PoolId =
                        pool_bytes.try_into().map_err(|_| anyhow!("Invalid pool id length"))?;
                    let vote = parse_vote(decoder)?;
                    votes.insert(pool_id, vote);
                }
            }
        },
    }

    Ok(votes)
}

/// Parse credential
///
/// credential = [0, addr_keyhash] / [1, script_hash]
fn parse_credential(decoder: &mut Decoder) -> Result<Credential> {
    decoder.array().context("Failed to parse credential array")?;
    let variant = decoder.u16().context("Failed to parse credential variant")?;

    match variant {
        0 => {
            let bytes = decoder.bytes().context("Failed to parse addr_keyhash")?;
            let hash: Hash<28> =
                bytes.try_into().map_err(|_| anyhow!("Invalid addr_keyhash length"))?;
            Ok(Credential::AddrKeyHash(hash))
        }
        1 => {
            let bytes = decoder.bytes().context("Failed to parse script_hash")?;
            let hash: Hash<28> =
                bytes.try_into().map_err(|_| anyhow!("Invalid script_hash length"))?;
            Ok(Credential::ScriptHash(hash))
        }
        _ => Err(anyhow!("Invalid credential variant: {variant}")),
    }
}

/// Parse vote (0 = No, 1 = Yes, 2 = Abstain)
fn parse_vote(decoder: &mut Decoder) -> Result<Vote> {
    let vote_value = decoder.u8().context("Failed to parse vote")?;
    match vote_value {
        0 => Ok(Vote::No),
        1 => Ok(Vote::Yes),
        2 => Ok(Vote::Abstain),
        _ => Err(anyhow!("Invalid vote value: {vote_value}")),
    }
}

/// Parse proposal_procedure
///
/// proposal_procedure = [
///   proposal_procedure_deposit : coin,
///   proposal_procedure_return_address : reward_account,
///   proposal_procedure_gov_action : gov_action,
///   proposal_procedure_anchor : anchor,
/// ]
fn parse_proposal_procedure(
    decoder: &mut Decoder,
    gov_action_id: GovActionId,
) -> Result<ProposalProcedure> {
    decoder.array().context("Failed to parse proposal_procedure array")?;

    // Parse deposit [0]
    let deposit = decoder.u64().context("Failed to parse deposit")?;

    // Parse return_address [1] - reward_account is bytes
    let reward_account_bytes = decoder.bytes().context("Failed to parse reward_account")?;
    let reward_account = StakeAddress::from_binary(reward_account_bytes)
        .context("Failed to decode reward_account")?;

    // Parse gov_action [2]
    let gov_action = parse_gov_action(decoder).context("Failed to parse gov_action")?;

    // Parse anchor [3]
    let anchor = parse_anchor(decoder).context("Failed to parse anchor")?;

    Ok(ProposalProcedure {
        deposit,
        reward_account,
        gov_action_id,
        gov_action,
        anchor: crate::Anchor {
            url: anchor.url,
            data_hash: anchor.content_hash.to_vec(),
        },
    })
}

/// Parse gov_action
///
/// gov_action =
///   [0, prev, pparams_update, policy_hash] / ; ParameterChange
///   [1, prev, prot_ver] /                    ; HardForkInitiation
///   [2, withdrawals, policy_hash] /          ; TreasuryWithdrawals
///   [3, prev] /                              ; NoConfidence
///   [4, prev, old_members, new_members, threshold] / ; UpdateCommittee
///   [5, prev, constitution] /                ; NewConstitution
///   [6]                                      ; Information
fn parse_gov_action(decoder: &mut Decoder) -> Result<GovernanceAction> {
    decoder.array().context("Failed to parse gov_action array")?;

    let variant = decoder.u8().context("Failed to parse gov_action variant")?;

    match variant {
        0 => {
            // ParameterChange
            let previous_action_id = parse_null_strict_maybe_gov_action_id(decoder)?;
            let protocol_param_update = parse_pparams_update(decoder)?;
            let script_hash = parse_null_maybe_bytes(decoder)?;

            Ok(GovernanceAction::ParameterChange(ParameterChangeAction {
                previous_action_id,
                protocol_param_update: Box::new(protocol_param_update),
                script_hash,
            }))
        }
        1 => {
            // HardForkInitiation
            let previous_action_id = parse_null_strict_maybe_gov_action_id(decoder)?;
            let protocol_version = parse_prot_ver(decoder)?;

            Ok(GovernanceAction::HardForkInitiation(
                HardForkInitiationAction {
                    previous_action_id,
                    protocol_version,
                },
            ))
        }
        2 => {
            // TreasuryWithdrawals
            let rewards = parse_withdrawals_map(decoder)?;
            let script_hash = parse_null_maybe_bytes(decoder)?;

            Ok(GovernanceAction::TreasuryWithdrawals(
                TreasuryWithdrawalsAction {
                    rewards,
                    script_hash,
                },
            ))
        }
        3 => {
            // NoConfidence
            let previous_action_id = parse_null_strict_maybe_gov_action_id(decoder)?;
            Ok(GovernanceAction::NoConfidence(previous_action_id))
        }
        4 => {
            // UpdateCommittee
            let previous_action_id = parse_null_strict_maybe_gov_action_id(decoder)?;
            let removed_members = parse_credential_set(decoder)?;
            let new_members = parse_committee_members_map(decoder)?;
            let terms = parse_unit_interval(decoder)?;

            Ok(GovernanceAction::UpdateCommittee(UpdateCommitteeAction {
                previous_action_id,
                data: CommitteeChange {
                    removed_committee_members: removed_members,
                    new_committee_members: new_members,
                    terms,
                },
            }))
        }
        5 => {
            // NewConstitution
            let previous_action_id = parse_null_strict_maybe_gov_action_id(decoder)?;
            let new_constitution = parse_constitution(decoder)?;

            Ok(GovernanceAction::NewConstitution(NewConstitutionAction {
                previous_action_id,
                new_constitution,
            }))
        }
        6 => {
            // Information - no additional data
            Ok(GovernanceAction::Information)
        }
        _ => Err(anyhow!("Invalid gov_action variant: {variant}")),
    }
}

/// Parse null_strict_maybe<gov_action_id>
fn parse_null_strict_maybe_gov_action_id(decoder: &mut Decoder) -> Result<Option<GovActionId>> {
    match decoder.datatype()? {
        Type::Null => {
            decoder.skip()?;
            Ok(None)
        }
        _ => {
            let id = parse_gov_action_id(decoder)?;
            Ok(Some(id))
        }
    }
}

/// Parse null_maybe<bytes>
fn parse_null_maybe_bytes(decoder: &mut Decoder) -> Result<Option<Vec<u8>>> {
    match decoder.datatype()? {
        Type::Null => {
            decoder.skip()?;
            Ok(None)
        }
        _ => {
            let bytes = decoder.bytes()?.to_vec();
            Ok(Some(bytes))
        }
    }
}

/// Parse pparams_update map
fn parse_pparams_update(decoder: &mut Decoder) -> Result<ProtocolParamUpdate> {
    // pparams_update is a map of int -> any
    // For now, we just skip it and return empty update
    // TODO: Implement full pparams_update parsing if needed
    decoder.skip().context("Failed to skip pparams_update")?;
    Ok(ProtocolParamUpdate::default())
}

/// Parse prot_ver
///
/// prot_ver = [pv_major : version, pv_minor : int]
fn parse_prot_ver(decoder: &mut Decoder) -> Result<ProtocolVersion> {
    decoder.array().context("Failed to parse prot_ver array")?;
    let major = decoder.u64().context("Failed to parse pv_major")?;
    let minor = decoder.u64().context("Failed to parse pv_minor")?;
    Ok(ProtocolVersion { major, minor })
}

/// Parse withdrawals map
fn parse_withdrawals_map(decoder: &mut Decoder) -> Result<HashMap<Vec<u8>, Lovelace>> {
    let mut withdrawals = HashMap::new();

    let map_len = decoder.map().context("Failed to parse withdrawals map")?;

    match map_len {
        Some(len) => {
            for _ in 0..len {
                let reward_account = decoder.bytes()?.to_vec();
                let amount = decoder.u64()?;
                withdrawals.insert(reward_account, amount);
            }
        }
        None => loop {
            match decoder.datatype()? {
                Type::Break => {
                    decoder.skip()?;
                    break;
                }
                _ => {
                    let reward_account = decoder.bytes()?.to_vec();
                    let amount = decoder.u64()?;
                    withdrawals.insert(reward_account, amount);
                }
            }
        },
    }

    Ok(withdrawals)
}

/// Parse set of credentials
fn parse_credential_set(decoder: &mut Decoder) -> Result<std::collections::HashSet<Credential>> {
    let mut set = std::collections::HashSet::new();

    // Sets might be tagged with CBOR tag 258
    if matches!(decoder.datatype()?, Type::Tag) {
        decoder.tag()?;
    }

    let arr_len = decoder.array().context("Failed to parse credential set")?;

    match arr_len {
        Some(len) => {
            for _ in 0..len {
                let credential = parse_credential(decoder)?;
                set.insert(credential);
            }
        }
        None => loop {
            match decoder.datatype()? {
                Type::Break => {
                    decoder.skip()?;
                    break;
                }
                _ => {
                    let credential = parse_credential(decoder)?;
                    set.insert(credential);
                }
            }
        },
    }

    Ok(set)
}

/// Parse committee members map (credential -> epoch)
fn parse_committee_members_map(decoder: &mut Decoder) -> Result<HashMap<Credential, u64>> {
    let mut members = HashMap::new();

    let map_len = decoder.map().context("Failed to parse committee members map")?;

    match map_len {
        Some(len) => {
            for _ in 0..len {
                let credential = parse_credential(decoder)?;
                let epoch = decoder.u64()?;
                members.insert(credential, epoch);
            }
        }
        None => loop {
            match decoder.datatype()? {
                Type::Break => {
                    decoder.skip()?;
                    break;
                }
                _ => {
                    let credential = parse_credential(decoder)?;
                    let epoch = decoder.u64()?;
                    members.insert(credential, epoch);
                }
            }
        },
    }

    Ok(members)
}

/// Parse unit_interval (rational number)
fn parse_unit_interval(decoder: &mut Decoder) -> Result<RationalNumber> {
    // unit_interval = #6.30([int, int])
    if matches!(decoder.datatype()?, Type::Tag) {
        decoder.tag()?;
    }

    decoder.array().context("Failed to parse unit_interval")?;
    let numerator = decoder.u64().context("Failed to parse numerator")?;
    let denominator = decoder.u64().context("Failed to parse denominator")?;

    Ok(RationalNumber::from(numerator, denominator))
}

/// Parse anchor
fn parse_anchor(decoder: &mut Decoder) -> Result<Anchor> {
    decoder.array().context("Failed to parse anchor array")?;

    // URL can be bytes or text
    let url = match decoder.datatype()? {
        Type::Bytes => {
            let bytes = decoder.bytes()?;
            String::from_utf8_lossy(bytes).to_string()
        }
        Type::String => decoder.str()?.to_string(),
        _ => return Err(anyhow!("Expected bytes or string for anchor URL")),
    };

    let content_hash_bytes = decoder.bytes().context("Failed to parse anchor hash")?;
    let content_hash: Hash<32> =
        content_hash_bytes.try_into().map_err(|_| anyhow!("Invalid anchor hash length"))?;

    Ok(Anchor { url, content_hash })
}

/// Parse constitution
///
/// constitution = [anchor, null_strict_maybe<script_hash>]
fn parse_constitution(decoder: &mut Decoder) -> Result<Constitution> {
    decoder.array().context("Failed to parse constitution array")?;

    // Parse anchor - check if flattened or wrapped
    let anchor = match decoder.datatype()? {
        Type::Bytes | Type::String => {
            // Flattened format: [url, data_hash, guardrail_script]
            let url = match decoder.datatype()? {
                Type::Bytes => {
                    let bytes = decoder.bytes()?;
                    String::from_utf8_lossy(bytes).to_string()
                }
                Type::String => decoder.str()?.to_string(),
                _ => return Err(anyhow!("Expected bytes or string for anchor URL")),
            };
            let data_hash = decoder.bytes()?.to_vec();
            crate::Anchor { url, data_hash }
        }
        Type::Array => {
            // Wrapped format: [[url, data_hash], guardrail_script]
            let anchor = parse_anchor(decoder)?;
            crate::Anchor {
                url: anchor.url,
                data_hash: anchor.content_hash.to_vec(),
            }
        }
        t => return Err(anyhow!("Unexpected type for constitution anchor: {t:?}")),
    };

    // Parse guardrail_script
    let guardrail_script = match decoder.datatype()? {
        Type::Null => {
            decoder.skip()?;
            None
        }
        _ => {
            let bytes = decoder.bytes()?;
            let hash: Hash<28> =
                bytes.try_into().map_err(|_| anyhow!("Invalid guardrail script hash length"))?;
            Some(hash)
        }
    };

    Ok(Constitution {
        anchor,
        guardrail_script,
    })
}

/// Parse strict_maybe<committee>
fn parse_strict_maybe_committee(decoder: &mut Decoder) -> Result<Option<Committee>> {
    let len = decoder.array().context("Failed to parse strict_maybe")?;

    match len {
        Some(0) => Ok(None),
        _ => {
            let committee = parse_committee(decoder)?;
            Ok(Some(committee))
        }
    }
}

/// Parse committee
///
/// committee = [
///   committee_members : { * credential_coldcommitteerole => int },
///   committee_threshold : unit_interval
/// ]
fn parse_committee(decoder: &mut Decoder) -> Result<Committee> {
    decoder.array().context("Failed to parse committee array")?;

    let members = parse_committee_members_map(decoder)?;
    let threshold = parse_unit_interval(decoder)?;

    Ok(Committee { members, threshold })
}

/// Parse drep_pulsing_state
///
/// drep_pulsing_state = [
///   drep_ps_snapshot : pulsing_snapshot,
///   drep_ps_ratify_state : ratify_state
/// ]
fn parse_drep_pulsing_state(decoder: &mut Decoder) -> Result<DRepPulsingResult> {
    decoder.array().context("Failed to parse drep_pulsing_state array")?;

    // Parse pulsing_snapshot [0]
    let votes = parse_pulsing_snapshot(decoder).context("Failed to parse pulsing_snapshot")?;

    // Parse ratify_state [1]
    let (enacted_actions, expired_action_ids, enacted_withdrawals) =
        parse_ratify_state(decoder).context("Failed to parse ratify_state")?;

    Ok((votes, enacted_actions, expired_action_ids, enacted_withdrawals))
}

/// Parse pulsing_snapshot
///
/// pulsing_snapshot = [
///   ps_proposals : strictseq<gov_action_state>,
///   ps_drep_distribution : { * drep => compactform_coin },
///   ps_drep_state : { * credential_x => drep_state },
///   ps_pool_distribution : { * key_hash<stake_pool> => compactform_coin }
/// ]
fn parse_pulsing_snapshot(
    decoder: &mut Decoder,
) -> Result<HashMap<GovActionId, HashMap<Voter, VotingProcedure>>> {
    decoder.array().context("Failed to parse pulsing_snapshot array")?;

    // Parse ps_proposals [0] - extract votes from gov_action_states
    let mut all_votes: HashMap<GovActionId, HashMap<Voter, VotingProcedure>> = HashMap::new();

    let props_len = decoder.array().context("Failed to parse ps_proposals")?;
    match props_len {
        Some(len) => {
            for _ in 0..len {
                if let Ok(state) = parse_gov_action_state(decoder) {
                    let mut proposal_votes = HashMap::new();

                    // Convert committee votes
                    for (credential, vote) in state.committee_votes {
                        let voter = match credential {
                            Credential::AddrKeyHash(hash) => {
                                Voter::ConstitutionalCommitteeKey(hash.into())
                            }
                            Credential::ScriptHash(hash) => {
                                Voter::ConstitutionalCommitteeScript(hash.into())
                            }
                        };
                        proposal_votes.insert(
                            voter,
                            VotingProcedure {
                                vote,
                                anchor: None,
                                vote_index: 0,
                            },
                        );
                    }

                    // Convert drep votes
                    for (credential, vote) in state.drep_votes {
                        let voter = match credential {
                            Credential::AddrKeyHash(hash) => Voter::DRepKey(hash.into()),
                            Credential::ScriptHash(hash) => Voter::DRepScript(hash.into()),
                        };
                        proposal_votes.insert(
                            voter,
                            VotingProcedure {
                                vote,
                                anchor: None,
                                vote_index: 0,
                            },
                        );
                    }

                    // Convert pool votes
                    for (pool_id, vote) in state.stake_pool_votes {
                        let voter = Voter::StakePoolKey(pool_id);
                        proposal_votes.insert(
                            voter,
                            VotingProcedure {
                                vote,
                                anchor: None,
                                vote_index: 0,
                            },
                        );
                    }

                    if !proposal_votes.is_empty() {
                        all_votes.insert(state.id, proposal_votes);
                    }
                } else {
                    decoder.skip()?;
                }
            }
        }
        None => {
            loop {
                match decoder.datatype()? {
                    Type::Break => {
                        decoder.skip()?;
                        break;
                    }
                    _ => {
                        decoder.skip()?; // Skip each entry for indefinite array
                    }
                }
            }
        }
    }

    // Skip remaining pulsing_snapshot fields [1], [2], [3]
    decoder.skip().context("Failed to skip ps_drep_distribution")?;
    decoder.skip().context("Failed to skip ps_drep_state")?;
    decoder.skip().context("Failed to skip ps_pool_distribution")?;

    Ok(all_votes)
}

/// Parse enact_state and extract es_withdrawals
///
/// enact_state = [
///   es_committee: strict_maybe<committee>,       [0]
///   es_constitution: constitution,               [1]
///   es_current_pparams: pparams,                 [2]
///   es_previous_pparams: pparams,                [3]
///   es_treasury: coin,                           [4]
///   es_withdrawals: { * credential => coin },    [5]
///   es_prev_gov_action_ids: gov_relation,        [6]
/// ]
fn parse_enact_state_withdrawals(decoder: &mut Decoder) -> Result<HashMap<Credential, Lovelace>> {
    decoder.array().context("Failed to parse enact_state array")?;

    // Skip es_committee [0]
    decoder.skip().context("Failed to skip es_committee")?;

    // Skip es_constitution [1]
    decoder.skip().context("Failed to skip es_constitution")?;

    // Skip es_current_pparams [2]
    decoder.skip().context("Failed to skip es_current_pparams")?;

    // Skip es_previous_pparams [3]
    decoder.skip().context("Failed to skip es_previous_pparams")?;

    // Skip es_treasury [4]
    decoder.skip().context("Failed to skip es_treasury")?;

    // Parse es_withdrawals [5] - map of credential to coin
    let mut withdrawals = HashMap::new();
    let map_len = decoder.map().context("Failed to parse es_withdrawals map")?;

    match map_len {
        Some(len) => {
            for _ in 0..len {
                if let Ok(credential) = parse_credential(decoder) {
                    let amount: Lovelace =
                        decoder.decode().context("Failed to parse withdrawal amount")?;
                    withdrawals.insert(credential, amount);
                } else {
                    // Skip credential we couldn't parse
                    decoder.skip()?;
                    decoder.skip()?;
                }
            }
        }
        None => {
            // Indefinite-length map
            loop {
                match decoder.datatype()? {
                    Type::Break => {
                        decoder.skip()?;
                        break;
                    }
                    _ => {
                        if let Ok(credential) = parse_credential(decoder) {
                            let amount: Lovelace =
                                decoder.decode().context("Failed to parse withdrawal amount")?;
                            withdrawals.insert(credential, amount);
                        } else {
                            decoder.skip()?;
                            decoder.skip()?;
                        }
                    }
                }
            }
        }
    }

    // Skip es_prev_gov_action_ids [6]
    decoder.skip().context("Failed to skip es_prev_gov_action_ids")?;

    if !withdrawals.is_empty() {
        info!(
            "Parsed {} enacted treasury withdrawals totaling {} ADA",
            withdrawals.len(),
            withdrawals.values().sum::<Lovelace>() / 1_000_000
        );
    }

    Ok(withdrawals)
}

/// Parse ratify_state
///
/// ratify_state = [
///   rs_enact_state : enact_state,
///   rs_enacted: [* gov_action_state],
///   rs_expired: set<gov_action_id>,
///   rs_delayed: bool
/// ]
///
/// Returns: (enacted_actions, expired_action_ids, enacted_withdrawals)
fn parse_ratify_state(
    decoder: &mut Decoder,
) -> Result<(Vec<GovActionState>, Vec<GovActionId>, HashMap<Credential, Lovelace>)> {
    decoder.array().context("Failed to parse ratify_state array")?;

    // Parse enact_state [0] to extract withdrawals
    let enacted_withdrawals =
        parse_enact_state_withdrawals(decoder).context("Failed to parse rs_enact_state")?;

    // Parse enacted [1]
    let mut enacted = Vec::new();
    let enacted_len = decoder.array().context("Failed to parse rs_enacted")?;
    match enacted_len {
        Some(len) => {
            for _ in 0..len {
                if let Ok(state) = parse_gov_action_state(decoder) {
                    enacted.push(state);
                } else {
                    decoder.skip()?;
                }
            }
        }
        None => loop {
            match decoder.datatype()? {
                Type::Break => {
                    decoder.skip()?;
                    break;
                }
                _ => {
                    decoder.skip()?;
                }
            }
        },
    }

    // Parse expired [2] - set<gov_action_id>
    let mut expired = Vec::new();

    // Sets might be tagged with CBOR tag 258
    if matches!(decoder.datatype()?, Type::Tag) {
        decoder.tag()?;
    }

    let expired_len = decoder.array().context("Failed to parse rs_expired")?;
    match expired_len {
        Some(len) => {
            for _ in 0..len {
                if let Ok(id) = parse_gov_action_id(decoder) {
                    expired.push(id);
                } else {
                    decoder.skip()?;
                }
            }
        }
        None => loop {
            match decoder.datatype()? {
                Type::Break => {
                    decoder.skip()?;
                    break;
                }
                _ => {
                    decoder.skip()?;
                }
            }
        },
    }

    // Skip delayed [3]
    decoder.skip().context("Failed to skip rs_delayed")?;

    Ok((enacted, expired, enacted_withdrawals))
}

// ============================================================================
// Conversion to ConwayVoting Types
// ============================================================================

impl GovernanceState {
    /// Convert parsed governance state to data needed for ConwayVoting bootstrap
    pub fn to_conway_voting_data(&self, _current_epoch: u64) -> ConwayVotingData {
        // Convert proposals to (epoch, ProposalProcedure) pairs
        let proposals: Vec<(u64, ProposalProcedure)> = self
            .proposals
            .iter()
            .map(|state| (state.proposed_in, state.proposal_procedure.clone()))
            .collect();

        // Merge votes from proposals and drep_pulsing_state
        let mut all_votes = self.votes.clone();

        for state in &self.proposals {
            let proposal_votes = all_votes.entry(state.id.clone()).or_default();

            // Add committee votes
            for (credential, vote) in &state.committee_votes {
                let voter = match credential {
                    Credential::AddrKeyHash(hash) => {
                        Voter::ConstitutionalCommitteeKey((*hash).into())
                    }
                    Credential::ScriptHash(hash) => {
                        Voter::ConstitutionalCommitteeScript((*hash).into())
                    }
                };
                proposal_votes.insert(
                    voter,
                    VotingProcedure {
                        vote: vote.clone(),
                        anchor: None,
                        vote_index: 0,
                    },
                );
            }

            // Add drep votes
            for (credential, vote) in &state.drep_votes {
                let voter = match credential {
                    Credential::AddrKeyHash(hash) => Voter::DRepKey((*hash).into()),
                    Credential::ScriptHash(hash) => Voter::DRepScript((*hash).into()),
                };
                proposal_votes.insert(
                    voter,
                    VotingProcedure {
                        vote: vote.clone(),
                        anchor: None,
                        vote_index: 0,
                    },
                );
            }

            // Add pool votes
            for (pool_id, vote) in &state.stake_pool_votes {
                let voter = Voter::StakePoolKey(*pool_id);
                proposal_votes.insert(
                    voter,
                    VotingProcedure {
                        vote: vote.clone(),
                        anchor: None,
                        vote_index: 0,
                    },
                );
            }
        }

        (proposals, all_votes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vote_values() {
        assert_eq!(parse_vote(&mut Decoder::new(&[0])).unwrap(), Vote::No);
        assert_eq!(parse_vote(&mut Decoder::new(&[1])).unwrap(), Vote::Yes);
        assert_eq!(parse_vote(&mut Decoder::new(&[2])).unwrap(), Vote::Abstain);
    }
}
