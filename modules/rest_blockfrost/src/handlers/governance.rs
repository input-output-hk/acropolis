//! REST handlers for Acropolis Blockfrost /governance endpoints
use crate::handlers_config::HandlersConfig;
use crate::types::{
    DRepInfoREST, DRepMetadataREST, DRepUpdateREST, DRepVoteREST, DRepsListREST, ProposalVoteREST,
    VoterRoleREST,
};
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        governance::{GovernanceStateQuery, GovernanceStateQueryResponse},
    },
    Credential, GovActionId, KeyHash, TxHash, Voter,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;
use reqwest::Client;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

pub async fn handle_dreps_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepsList,
    )));

    let raw_msg = context.message_bus.request(&handlers_config.dreps_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());
    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepsList(list),
        )) => {
            let response: Vec<DRepsListREST> = list
                .dreps
                .iter()
                .map(|cred| {
                    Ok(DRepsListREST {
                        drep_id: cred.to_drep_bech32()?,
                        hex: hex::encode(cred.get_hash()),
                    })
                })
                .collect::<Result<_, anyhow::Error>>()?;

            match serde_json::to_string_pretty(&response) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to serialize response: {e}"),
                )),
            }
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
    handlers_config: Arc<HandlersConfig>,
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
        GovernanceStateQuery::GetDRepInfoWithDelegators {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context.message_bus.request(&handlers_config.dreps_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepInfoWithDelegators(response),
        )) => {
            let active = !response.info.retired && !response.info.expired;

            let stake_addresses = response.delegators.clone();

            let sum_msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
                AccountsStateQuery::GetAccountsBalancesSum { stake_addresses },
            )));

            let raw_sum =
                context.message_bus.request(&handlers_config.accounts_query_topic, sum_msg).await?;
            let sum_response = Arc::try_unwrap(raw_sum).unwrap_or_else(|arc| (*arc).clone());

            let amount = match sum_response {
                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::AccountsBalancesSum(sum),
                )) => sum.to_string(),

                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::Error(e),
                )) => {
                    return Ok(RESTResponse::with_text(
                        500,
                        &format!("Failed to sum balances: {e}"),
                    ));
                }

                _ => {
                    return Ok(RESTResponse::with_text(
                        500,
                        "Unexpected response from accounts-state",
                    ));
                }
            };

            let response = DRepInfoREST {
                drep_id: drep_id.to_string(),
                hex: hex::encode(credential.get_hash()),
                amount,
                active,
                active_epoch: response.info.active_epoch,
                has_script: matches!(credential, Credential::ScriptHash(_)),
                last_active_epoch: response.info.last_active_epoch,
                retired: response.info.retired,
                expired: response.info.expired,
            };

            match serde_json::to_string_pretty(&response) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to serialize DRep info: {e}"),
                )),
            }
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "DRep not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Ok(RESTResponse::with_text(500, &format!("{e}"))),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_drep_delegators_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(drep_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing DRep ID parameter"));
    };

    let credential = match parse_drep_credential(drep_id) {
        Ok(c) => c,
        Err(resp) => return Ok(resp),
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepDelegators {
            drep_credential: credential,
        },
    )));

    let raw_msg = context.message_bus.request(&handlers_config.dreps_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepDelegators(delegators),
        )) => {
            let stake_key_to_bech32: HashMap<KeyHash, String> = delegators
                .addresses
                .iter()
                .map(|addr| {
                    let bech32 = addr
                        .get_credential()
                        .to_stake_bech32()
                        .map_err(|_| anyhow!("Failed to encode stake address"))?;
                    let key_hash = addr.get_hash().to_vec();
                    Ok((key_hash, bech32))
                })
                .collect::<Result<HashMap<_, _>>>()?;

            let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
                AccountsStateQuery::GetAccountsUtxoValuesMap {
                    stake_addresses: delegators.addresses.clone(),
                },
            )));

            let raw_msg =
                context.message_bus.request(&handlers_config.accounts_query_topic, msg).await?;
            let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

            match message {
                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::AccountsUtxoValuesMap(map),
                )) => {
                    let mut response = Vec::new();

                    for (key, amount) in map {
                        let Some(bech32) = stake_key_to_bech32.get(&key) else {
                            return Ok(RESTResponse::with_text(
                                500,
                                "Internal error: missing Bech32 for stake key",
                            ));
                        };

                        response.push(serde_json::json!({
                            "address": bech32,
                            "amount": amount.to_string(),
                        }));
                    }

                    match serde_json::to_string_pretty(&response) {
                        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                        Err(e) => Ok(RESTResponse::with_text(
                            500,
                            &format!("Failed to serialize DRep delegators: {e}"),
                        )),
                    }
                }

                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::Error(e),
                )) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Account state error: {e}"),
                )),

                _ => Ok(RESTResponse::with_text(
                    500,
                    "Unexpected response from accounts-state",
                )),
            }
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "DRep not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(_),
        )) => Ok(RESTResponse::with_text(
            500,
            "DRep delegator storage is disabled in config",
        )),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_drep_metadata_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(drep_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing DRep ID parameter"));
    };

    let credential = match parse_drep_credential(drep_id) {
        Ok(c) => c,
        Err(resp) => return Ok(resp),
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepMetadata {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context.message_bus.request(&handlers_config.dreps_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepMetadata(metadata),
        )) => {
            match metadata {
                None => {
                    // metadata feature disabled
                    Ok(RESTResponse::with_text(
                        500,
                        "DRep metadata storage is disabled in config",
                    ))
                }
                Some(None) => {
                    // enabled, but nothing stored for this DRep
                    Ok(RESTResponse::with_text(404, "DRep metadata not found"))
                }
                Some(Some(anchor)) => {
                    // enabled + stored → fetch the JSON
                    match Client::new().get(&anchor.url).send().await {
                        Ok(resp) => match resp.bytes().await {
                            Ok(raw_bytes) => match serde_json::from_slice::<Value>(&raw_bytes) {
                                Ok(json) => {
                                    let bytes_hex = format!("\\x{}", hex::encode(&raw_bytes));

                                    let response = DRepMetadataREST {
                                        drep_id: drep_id.to_string(),
                                        hex: hex::encode(credential.get_hash()),
                                        url: anchor.url.clone(),
                                        hash: hex::encode(anchor.data_hash.clone()),
                                        json_metadata: json,
                                        bytes: bytes_hex,
                                    };

                                    match serde_json::to_string_pretty(&response) {
                                        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                                        Err(e) => Ok(RESTResponse::with_text(
                                            500,
                                            &format!("Failed to serialize DRep metadata: {e}"),
                                        )),
                                    }
                                }
                                Err(_) => Ok(RESTResponse::with_text(
                                    500,
                                    "Invalid JSON from DRep metadata URL",
                                )),
                            },
                            Err(_) => Ok(RESTResponse::with_text(
                                500,
                                "Failed to read bytes from DRep metadata URL",
                            )),
                        },
                        Err(_) => Ok(RESTResponse::with_text(
                            500,
                            "Failed to fetch DRep metadata URL",
                        )),
                    }
                }
            }
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "DRep metadata not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(_),
        )) => Ok(RESTResponse::with_text(
            500,
            "DRep metadata storage is disabled in config",
        )),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_drep_updates_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(drep_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing DRep ID parameter"));
    };

    let credential = match parse_drep_credential(drep_id) {
        Ok(c) => c,
        Err(resp) => return Ok(resp),
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepUpdates {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context.message_bus.request(&handlers_config.dreps_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepUpdates(list),
        )) => {
            let response: Vec<DRepUpdateREST> = list
                .updates
                .iter()
                .map(|event| DRepUpdateREST {
                    tx_hash: hex::encode(event.tx_hash),
                    cert_index: event.cert_index,
                    action: event.action.clone(),
                })
                .collect();

            match serde_json::to_string_pretty(&response) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to serialize DRep updates: {e}"),
                )),
            }
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(_),
        )) => Ok(RESTResponse::with_text(
            503,
            &format!("DRep updates storage is disabled in config"),
        )),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "DRep not found")),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_drep_votes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(drep_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing DRep ID parameter"));
    };

    let credential = match parse_drep_credential(drep_id) {
        Ok(c) => c,
        Err(resp) => return Ok(resp),
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepVotes {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context.message_bus.request(&handlers_config.dreps_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());
    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepVotes(votes),
        )) => {
            let response: Vec<_> = votes
                .votes
                .iter()
                .map(|vote| DRepVoteREST {
                    tx_hash: hex::encode(&vote.tx_hash),
                    cert_index: vote.vote_index,
                    vote: vote.vote.clone(),
                })
                .collect();

            match serde_json::to_string_pretty(&response) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to serialize DRep votes: {e}"),
                )),
            }
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Ok(RESTResponse::with_text(404, "DRep not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(_),
        )) => Ok(RESTResponse::with_text(
            503,
            "DRep vote storage is disabled in config",
        )),

        _ => Ok(RESTResponse::with_text(500, "Unexpected message type")),
    }
}

pub async fn handle_proposals_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetProposalsList,
    )));

    let raw_msg = context.message_bus.request(&handlers_config.governance_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

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
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let proposal = match parse_gov_action_id(&params)? {
        Ok(id) => id,
        Err(resp) => return Ok(resp),
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetProposalInfo { proposal },
    )));

    let raw_msg = context.message_bus.request(&handlers_config.governance_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

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
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_proposal_withdrawals_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_proposal_votes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let proposal = match parse_gov_action_id(&params)? {
        Ok(id) => id,
        Err(resp) => return Ok(resp),
    };

    let tx_hash = hex::encode(&proposal.transaction_id);
    let cert_index = proposal.action_index;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetProposalVotes { proposal },
    )));

    let raw_msg = context.message_bus.request(&handlers_config.governance_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::ProposalVotes(votes),
        )) => {
            let mut votes_list = Vec::new();

            for (voter, (_, voting_proc)) in votes.votes {
                let voter_role = match voter {
                    Voter::ConstitutionalCommitteeKey(_)
                    | Voter::ConstitutionalCommitteeScript(_) => {
                        VoterRoleREST::ConstitutionalCommittee
                    }
                    Voter::DRepKey(_) | Voter::DRepScript(_) => VoterRoleREST::Drep,
                    Voter::StakePoolKey(_) => VoterRoleREST::Spo,
                };

                let voter_str = voter.to_string();

                votes_list.push(ProposalVoteREST {
                    tx_hash: tx_hash.clone(),
                    cert_index,
                    voter_role,
                    voter: voter_str,
                    vote: voting_proc.vote,
                });
            }

            match serde_json::to_string(&votes_list) {
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
    _handlers_config: Arc<HandlersConfig>,
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

    let transaction_id: TxHash = match hex::decode(tx_hash_hex) {
        Ok(bytes) => match bytes.as_slice().try_into() {
            Ok(arr) => arr,
            Err(_) => {
                return Ok(Err(RESTResponse::with_text(
                    400,
                    "Invalid tx_hash length, must be 32 bytes",
                )));
            }
        },
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

fn parse_drep_credential(drep_id: &str) -> Result<Credential, RESTResponse> {
    Credential::from_drep_bech32(drep_id).map_err(|e| {
        RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 DRep ID: {drep_id}. Error: {e}"),
        )
    })
}
