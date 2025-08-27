//! REST handlers for Acropolis Blockfrost /governance endpoints
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::governance::{GovernanceStateQuery, GovernanceStateQueryResponse},
    Credential, GovActionId,
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::{collections::BTreeMap, sync::Arc};

use crate::types::VoteRest;

pub async fn handle_dreps_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepsList,
    )));
    let raw = context.message_bus.request("cardano.query.dreps", msg).await?;
    let message = Arc::try_unwrap(raw).unwrap_or_else(|arc| (*arc).clone());
    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepsList(list),
        )) => {
            let dreps: Vec<String> =
                list.dreps.iter().map(|cred| cred.to_drep_bech32()).collect::<Result<_, _>>()?;

            Ok(RESTResponse::with_json(
                200,
                &serde_json::to_string(&dreps)?,
            ))
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Ok(RESTResponse::with_text(500, &format!("Query error: {e}"))),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "No DReps found")),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_single_drep_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
) -> Result<RESTResponse> {
    let Some(drep_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing DRep ID parameter"));
    };

    let credential = match Credential::from_drep_bech32(drep_id) {
        Ok(c) => c,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid Bech32 DRep ID: {drep_id}. Error: {e}"),
            ));
        }
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepInfo {
            drep_credential: credential,
        },
    )));

    let raw = context.message_bus.request("cardano.query.dreps", msg).await?;
    let message = Arc::try_unwrap(raw).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepInfo(info),
        )) => match serde_json::to_string(&info) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Failed to serialize DRep info: {e}"),
            )),
        },

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "DRep not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Ok(RESTResponse::with_text(500, &format!("Query error: {e}"))),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_drep_delegators_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_drep_metadata_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_drep_updates_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_drep_votes_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_proposals_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetProposalsList,
    )));

    let raw = context.message_bus.request("governance-state", msg).await?;
    let message = Arc::try_unwrap(raw).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::ProposalsList(list),
        )) => {
            if list.proposals.is_empty() {
                return Ok(RESTResponse::with_json(200, "[]"));
            }

            let props_bech32: Result<Vec<String>, _> =
                list.proposals.iter().map(|id| id.to_bech32()).collect();

            match props_bech32 {
                Ok(vec) => match serde_json::to_string(&vec) {
                    Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                    Err(e) => Ok(RESTResponse::with_text(
                        500,
                        &format!("Failed to serialize proposals list: {e}"),
                    )),
                },
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to convert proposal IDs to Bech32: {e}"),
                )),
            }
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Ok(RESTResponse::with_text(500, &format!("Query error: {e}"))),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "No proposals found")),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_single_proposal_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
) -> Result<RESTResponse> {
    let proposal = match parse_gov_action_id(&params)? {
        Ok(id) => id,
        Err(resp) => return Ok(resp),
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetProposalInfo { proposal },
    )));
    let raw = context.message_bus.request("governance-state", msg).await?;
    let message = Arc::try_unwrap(raw).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::ProposalInfo(info),
        )) => match serde_json::to_string(&info) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Failed to serialize proposal info: {e}"),
            )),
        },

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "Proposal not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Ok(RESTResponse::with_text(500, &format!("Query error: {e}"))),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_proposal_parameters_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_proposal_withdrawals_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_proposal_votes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
) -> Result<RESTResponse> {
    let proposal = match parse_gov_action_id(&params)? {
        Ok(id) => id,
        Err(resp) => return Ok(resp),
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetProposalVotes { proposal },
    )));

    let raw = context.message_bus.request("governance-state", msg).await?;
    let message = Arc::try_unwrap(raw).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::ProposalVotes(votes),
        )) => {
            let mut votes_map = BTreeMap::new();

            for (voter, (data_hash, voting_proc)) in votes.votes {
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

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "Proposal not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Ok(RESTResponse::with_text(500, &format!("Query error: {e}"))),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_proposal_metadata_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub fn parse_gov_action_id(params: &[String]) -> Result<Result<GovActionId, RESTResponse>> {
    if params.len() != 2 {
        return Ok(Err(RESTResponse::with_text(
            400,
            "Expected two parameters: tx_hash/cert_index",
        )));
    }

    let tx_hash_hex = &params[0];
    let cert_index_str = &params[1];

    let transaction_id = match hex::decode(tx_hash_hex) {
        Ok(bytes) => bytes,
        Err(e) => {
            return Ok(Err(RESTResponse::with_text(
                400,
                &format!("Invalid hex tx_hash: {e}"),
            )));
        }
    };

    let action_index = match cert_index_str.parse::<u8>() {
        Ok(i) => i,
        Err(e) => {
            return Ok(Err(RESTResponse::with_text(
                400,
                &format!("Invalid cert_index, expected u8: {e}"),
            )));
        }
    };

    Ok(Ok(GovActionId {
        transaction_id,
        action_index,
    }))
}
