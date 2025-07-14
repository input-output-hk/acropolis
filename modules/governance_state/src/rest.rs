//! REST handlers for Acropolis Governance State module

use std::{collections::BTreeMap, sync::Arc};

use acropolis_common::{
    messages::RESTResponse, serialization::ToBech32WithHrp, Anchor, GovActionId, GovernanceAction,
    VotingProcedure,
};
use anyhow::Result;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::state::State;

/// REST response structure for proposal info
#[derive(Serialize)]
pub struct ProposalProcedureRest {
    pub deposit: u64,
    pub reward_account: String,
    pub gov_action_id: String,
    pub gov_action: GovernanceAction,
    pub anchor: Anchor,
}

/// REST response structure for proposal votes
#[derive(Serialize)]
pub struct VoteRest {
    pub transaction: String,
    pub voting_procedure: VotingProcedure,
}

/// Handles /governance/list
pub async fn handle_list(state: Arc<Mutex<State>>) -> Result<RESTResponse> {
    let locked = state.lock().await;
    let props_bech32: Result<Vec<String>, anyhow::Error> =
        locked.list_proposals().iter().map(|id| id.to_bech32()).collect();

    match props_bech32 {
        Ok(vec) => match serde_json::to_string(&vec) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving governance list: {e}"),
            )),
        },
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving governance list: {e}"),
        )),
    }
}

/// Handles /governance/info/<Bech32_GovActionId>
pub async fn handle_proposal(
    state: Arc<Mutex<State>>,
    param_string: String,
) -> Result<RESTResponse> {
    let proposal_id = match GovActionId::from_bech32(&param_string) {
        Ok(id) => id,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid Bech32 governance proposal: {param_string}. Error: {e}"),
            ));
        }
    };

    let locked = state.lock().await;
    match locked.get_proposal(&proposal_id) {
        Some(proposal) => {
            let hrp = match proposal.reward_account.first() {
                Some(0xe0) => "stake_test",
                Some(0xe1) => "stake",
                _ => "stake",
            };

            let reward_account_bech32 = match proposal.reward_account.to_bech32_with_hrp(hrp) {
                Ok(val) => val,
                Err(e) => {
                    return Ok(RESTResponse::with_text(
                        500,
                        &format!("Internal server error while retrieving proposal: {e}"),
                    ));
                }
            };

            let gov_action_id_bech32 = match proposal.gov_action_id.to_bech32() {
                Ok(val) => val,
                Err(e) => {
                    return Ok(RESTResponse::with_text(
                        500,
                        &format!("Internal server error while retrieving proposal: {e}"),
                    ));
                }
            };

            let proposal_rest = ProposalProcedureRest {
                deposit: proposal.deposit,
                reward_account: reward_account_bech32,
                gov_action_id: gov_action_id_bech32,
                gov_action: proposal.gov_action.clone(),
                anchor: proposal.anchor.clone(),
            };

            match serde_json::to_string(&proposal_rest) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while retrieving proposal: {e}"),
                )),
            }
        }
        None => Ok(RESTResponse::with_text(404, "Proposal not found")),
    }
}

/// Handles /governance/votes/<Bech32_GovActionId>
pub async fn handle_votes(state: Arc<Mutex<State>>, param_string: String) -> Result<RESTResponse> {
    let proposal_id = match GovActionId::from_bech32(&param_string) {
        Ok(id) => id,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid Bech32 governance proposal: {param_string}. Error: {e}"),
            ));
        }
    };

    let locked = state.lock().await;
    match locked.get_proposal_votes(&proposal_id) {
        Ok(votes) => {
            let mut votes_map = BTreeMap::new();

            for (voter, (data_hash, voting_proc)) in votes {
                let voter_bech32 = voter.to_string();
                let transaction_hex = hex::encode(data_hash);

                votes_map.insert(
                    voter_bech32,
                    VoteRest {
                        transaction: transaction_hex,
                        voting_procedure: voting_proc,
                    },
                );
            }

            match serde_json::to_string(&votes_map) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while retrieving proposal votes: {e}"),
                )),
            }
        }
        Err(_) => Ok(RESTResponse::with_text(404, "Proposal not found")),
    }
}
