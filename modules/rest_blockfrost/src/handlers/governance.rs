//! REST handlers for Acropolis Blockfrost /governance endpoints
use crate::handlers_config::HandlersConfig;
use crate::types::{
    DRepInfoREST, DRepMetadataREST, DRepUpdateREST, DRepVoteREST, DRepsListREST, ProposalVoteREST,
    VoterRoleREST,
};
use acropolis_common::queries::errors::QueryError;
use acropolis_common::rest_error::RESTError;
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        governance::{GovernanceStateQuery, GovernanceStateQueryResponse},
    },
    Credential, GovActionId, TxHash, Voter,
};
use caryatid_sdk::Context;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_dreps_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepsList,
    )));

    let raw_msg = context
        .message_bus
        .request(&handlers_config.dreps_query_topic, msg)
        .await
        .map_err(|e| RESTError::InternalServerError(format!("Message bus error: {e}")))?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepsList(list),
        )) => {
            let response: Result<Vec<DRepsListREST>, RESTError> = list
                .dreps
                .iter()
                .map(|cred| {
                    Ok(DRepsListREST {
                        drep_id: cred
                            .to_drep_bech32()
                            .map_err(|e| RESTError::encoding_failed(&format!("DRep ID: {e}")))?,
                        hex: hex::encode(cred.get_hash()),
                    })
                })
                .collect();

            let response = response?;
            let json = serde_json::to_string_pretty(&response)?;
            Ok(RESTResponse::with_json(200, &json))
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(QueryError::NotFound { .. }),
        )) => Err(RESTError::not_found("No DReps found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(e.into()),

        _ => Err(RESTError::unexpected_response("Unexpected message type")),
    }
}

pub async fn handle_single_drep_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let Some(drep_id) = params.first() else {
        return Err(RESTError::param_missing("DRep ID"));
    };

    let credential = parse_drep_credential(drep_id)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepInfoWithDelegators {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context
        .message_bus
        .request(&handlers_config.dreps_query_topic, msg)
        .await
        .map_err(|e| RESTError::InternalServerError(format!("Message bus error: {e}")))?;
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

            let raw_sum = context
                .message_bus
                .request(&handlers_config.accounts_query_topic, sum_msg)
                .await
                .map_err(|e| RESTError::InternalServerError(format!("Message bus error: {e}")))?;
            let sum_response = Arc::try_unwrap(raw_sum).unwrap_or_else(|arc| (*arc).clone());

            let amount = match sum_response {
                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::AccountsBalancesSum(sum),
                )) => sum.to_string(),

                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::Error(e),
                )) => {
                    return Err(RESTError::InternalServerError(format!(
                        "Failed to sum balances: {e}"
                    )));
                }

                _ => {
                    return Err(RESTError::unexpected_response(
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

            let json = serde_json::to_string_pretty(&response)?;
            Ok(RESTResponse::with_json(200, &json))
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(QueryError::NotFound { .. }),
        )) => Err(RESTError::not_found("DRep not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(e.into()),

        _ => Err(RESTError::unexpected_response("Unexpected message type")),
    }
}

pub async fn handle_drep_delegators_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let Some(drep_id) = params.first() else {
        return Err(RESTError::param_missing("DRep ID"));
    };

    let credential = parse_drep_credential(drep_id)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepDelegators {
            drep_credential: credential,
        },
    )));

    let raw_msg = context
        .message_bus
        .request(&handlers_config.dreps_query_topic, msg)
        .await
        .map_err(|e| RESTError::InternalServerError(format!("Message bus error: {e}")))?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepDelegators(delegators),
        )) => {
            let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
                AccountsStateQuery::GetAccountsUtxoValuesMap {
                    stake_addresses: delegators.addresses.clone(),
                },
            )));

            let raw_msg = context
                .message_bus
                .request(&handlers_config.accounts_query_topic, msg)
                .await
                .map_err(|e| RESTError::InternalServerError(format!("Message bus error: {e}")))?;
            let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

            match message {
                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::AccountsUtxoValuesMap(map),
                )) => {
                    let response: Result<Vec<_>, RESTError> = map
                        .into_iter()
                        .map(|(stake_address, amount)| {
                            let bech32 = stake_address.to_string().map_err(|e| {
                                RESTError::InternalServerError(format!(
                                    "Failed to encode stake address: {}",
                                    e
                                ))
                            })?;

                            Ok(serde_json::json!({
                                "address": bech32,
                                "amount": amount.to_string(),
                            }))
                        })
                        .collect();

                    let response = response?;
                    let json = serde_json::to_string_pretty(&response)?;
                    Ok(RESTResponse::with_json(200, &json))
                }

                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::Error(e),
                )) => Err(e.into()),

                _ => Err(RESTError::unexpected_response(
                    "Unexpected response from accounts-state",
                )),
            }
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(QueryError::NotFound { .. }),
        )) => Err(RESTError::not_found("DRep not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(e.into()),

        _ => Err(RESTError::unexpected_response("Unexpected message type")),
    }
}

pub async fn handle_drep_metadata_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let Some(drep_id) = params.first() else {
        return Err(RESTError::param_missing("DRep ID"));
    };

    let credential = parse_drep_credential(drep_id)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepMetadata {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context
        .message_bus
        .request(&handlers_config.dreps_query_topic, msg)
        .await
        .map_err(|e| RESTError::InternalServerError(format!("Message bus error: {e}")))?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepMetadata(metadata),
        )) => match metadata {
            None => Err(RESTError::storage_disabled("DRep metadata")),
            Some(None) => Err(RESTError::not_found("DRep metadata not found")),
            Some(Some(anchor)) => {
                let resp = Client::new().get(&anchor.url).send().await.map_err(|_| {
                    RESTError::InternalServerError("Failed to fetch DRep metadata URL".to_string())
                })?;

                let raw_bytes = resp.bytes().await.map_err(|_| {
                    RESTError::InternalServerError(
                        "Failed to read bytes from DRep metadata URL".to_string(),
                    )
                })?;

                let json = serde_json::from_slice::<Value>(&raw_bytes).map_err(|_| {
                    RESTError::InternalServerError(
                        "Invalid JSON from DRep metadata URL".to_string(),
                    )
                })?;

                let bytes_hex = format!("\\x{}", hex::encode(&raw_bytes));

                let response = DRepMetadataREST {
                    drep_id: drep_id.to_string(),
                    hex: hex::encode(credential.get_hash()),
                    url: anchor.url.clone(),
                    hash: hex::encode(anchor.data_hash.clone()),
                    json_metadata: json,
                    bytes: bytes_hex,
                };

                let json = serde_json::to_string_pretty(&response)?;
                Ok(RESTResponse::with_json(200, &json))
            }
        },

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(QueryError::NotFound { .. }),
        )) => Err(RESTError::not_found("DRep metadata not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(e.into()),

        _ => Err(RESTError::unexpected_response("Unexpected message type")),
    }
}

pub async fn handle_drep_updates_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let Some(drep_id) = params.first() else {
        return Err(RESTError::param_missing("DRep ID"));
    };

    let credential = parse_drep_credential(drep_id)?;

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
                    tx_hash: "TxHash lookup not yet implemented".to_string(),
                    cert_index: event.cert_index,
                    action: event.action.clone(),
                })
                .collect();

            let json = serde_json::to_string_pretty(&response)?;
            Ok(RESTResponse::with_json(200, &json))
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(QueryError::NotFound { .. }),
        )) => Err(RESTError::not_found("DRep not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(e.into()),

        _ => Err(RESTError::unexpected_response("Unexpected message type")),
    }
}

pub async fn handle_drep_votes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let Some(drep_id) = params.first() else {
        return Err(RESTError::param_missing("DRep ID"));
    };

    let credential = parse_drep_credential(drep_id)?;

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
                    tx_hash: hex::encode(vote.tx_hash),
                    cert_index: vote.vote_index,
                    vote: vote.vote.clone(),
                })
                .collect();

            let json = serde_json::to_string_pretty(&response)?;
            Ok(RESTResponse::with_json(200, &json))
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(QueryError::NotFound { .. }),
        )) => Err(RESTError::not_found("DRep not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(e.into()),

        _ => Err(RESTError::unexpected_response("Unexpected message type")),
    }
}

pub async fn handle_proposals_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
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

            let vec = props_bech32
                .map_err(|e| RESTError::encoding_failed(&format!("proposal IDs to Bech32: {e}")))?;

            let json = serde_json::to_string(&vec)?;
            Ok(RESTResponse::with_json(200, &json))
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(QueryError::NotFound { .. }),
        )) => Err(RESTError::not_found("No proposals found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(e.into()),

        _ => Err(RESTError::unexpected_response("Unexpected message type")),
    }
}

pub async fn handle_single_proposal_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let proposal = parse_gov_action_id(&params)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetProposalInfo { proposal },
    )));

    let raw_msg = context.message_bus.request(&handlers_config.governance_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::ProposalInfo(info),
        )) => {
            let json = serde_json::to_string(&info)?;
            Ok(RESTResponse::with_json(200, &json))
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(QueryError::NotFound { .. }),
        )) => Err(RESTError::not_found("Proposal not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(e.into()),

        _ => Err(RESTError::unexpected_response("Unexpected message type")),
    }
}

pub async fn handle_proposal_parameters_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Proposal parameters endpoint"))
}

pub async fn handle_proposal_withdrawals_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Proposal withdrawals endpoint"))
}

pub async fn handle_proposal_votes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let proposal = parse_gov_action_id(&params)?;

    let tx_hash = hex::encode(proposal.transaction_id);
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

            let json = serde_json::to_string(&votes_list)?;
            Ok(RESTResponse::with_json(200, &json))
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(QueryError::NotFound { .. }),
        )) => Err(RESTError::not_found("Proposal not found")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(e.into()),

        _ => Err(RESTError::unexpected_response("Unexpected message type")),
    }
}

pub async fn handle_proposal_metadata_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Proposal metadata endpoint"))
}

pub fn parse_gov_action_id(params: &[String]) -> Result<GovActionId, RESTError> {
    if params.len() != 2 {
        return Err(RESTError::BadRequest(
            "Expected two parameters: tx_hash/cert_index".to_string(),
        ));
    }

    let tx_hash_hex = &params[0];
    let cert_index_str = &params[1];

    let bytes = hex::decode(tx_hash_hex)?;
    let transaction_id: TxHash = bytes.as_slice().try_into().map_err(|_| {
        RESTError::invalid_param("tx_hash", "invalid tx_hash length, must be 32 bytes")
    })?;

    let action_index = cert_index_str
        .parse::<u8>()
        .map_err(|_| RESTError::invalid_param("cert_index", "expected u8"))?;

    Ok(GovActionId {
        transaction_id,
        action_index,
    })
}

fn parse_drep_credential(drep_id: &str) -> Result<Credential, RESTError> {
    Credential::from_drep_bech32(drep_id)
        .map_err(|e| RESTError::invalid_param("drep_id", &format!("invalid Bech32 DRep ID: {e}")))
}
