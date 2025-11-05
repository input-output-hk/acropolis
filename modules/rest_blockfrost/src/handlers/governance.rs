//! REST handlers for Acropolis Blockfrost /governance endpoints
use crate::handlers_config::HandlersConfig;
use crate::types::{
    DRepInfoREST, DRepMetadataREST, DRepUpdateREST, DRepVoteREST, DRepsListREST, ProposalVoteREST,
    VoterRoleREST,
};
use acropolis_common::app_error::RESTError;
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        governance::{GovernanceStateQuery, GovernanceStateQueryResponse},
        utils::query_state,
    },
    Credential, GovActionId, TxHash, Voter,
};
use caryatid_sdk::Context;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use acropolis_common::serialization::serialize_to_json_response;

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
        .map_err(|e| RESTError::query_failed(e))?;

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
                        drep_id: cred
                            .to_drep_bech32()
                            .map_err(|e| RESTError::encoding_failed(&format!("DRep ID: {}", e)))?,
                        hex: hex::encode(cred.get_hash()),
                    })
                })
                .collect::<Result<_, RESTError>>()?;

            serialize_to_json_response(&response)
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(RESTError::query_failed(e)),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Err(RESTError::not_found("DReps")),

        _ => Err(RESTError::unexpected_response("retrieving DReps list")),
    }
}

pub async fn handle_single_drep_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = Credential::from_drep_bech32(drep_id)
        .map_err(|e| RESTError::invalid_param("drep_id", &e.to_string()))?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepInfoWithDelegators {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context
        .message_bus
        .request(&handlers_config.dreps_query_topic, msg)
        .await
        .map_err(|e| RESTError::query_failed(e))?;

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

            let amount = query_state(
                &context,
                &handlers_config.accounts_query_topic,
                sum_msg,
                |message| match message {
                    Message::StateQueryResponse(StateQueryResponse::Accounts(
                        AccountsStateQueryResponse::AccountsBalancesSum(sum),
                    )) => Ok(sum.to_string()),

                    Message::StateQueryResponse(StateQueryResponse::Accounts(
                        AccountsStateQueryResponse::Error(e),
                    )) => Err(RESTError::query_failed(format!(
                        "Failed to sum balances: {}",
                        e
                    ))),

                    _ => Err(RESTError::unexpected_response("summing account balances")),
                },
            )
            .await?;

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

            serialize_to_json_response(&response)
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Err(RESTError::not_found("DRep")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(RESTError::query_failed(e)),

        _ => Err(RESTError::unexpected_response("retrieving DRep info")),
    }
}

pub async fn handle_drep_delegators_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = Credential::from_drep_bech32(drep_id)
        .map_err(|e| RESTError::invalid_param("drep_id", &e.to_string()))?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepDelegators {
            drep_credential: credential,
        },
    )));

    let raw_msg = context
        .message_bus
        .request(&handlers_config.dreps_query_topic, msg)
        .await
        .map_err(|e| RESTError::query_failed(e))?;

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

            let map = query_state(
                &context,
                &handlers_config.accounts_query_topic,
                msg,
                |message| match message {
                    Message::StateQueryResponse(StateQueryResponse::Accounts(
                        AccountsStateQueryResponse::AccountsUtxoValuesMap(map),
                    )) => Ok(map),

                    Message::StateQueryResponse(StateQueryResponse::Accounts(
                        AccountsStateQueryResponse::Error(e),
                    )) => Err(RESTError::query_failed(format!(
                        "Account state error: {}",
                        e
                    ))),

                    _ => Err(RESTError::unexpected_response(
                        "retrieving accounts UTxO values",
                    )),
                },
            )
            .await?;

            let response: Vec<_> = map
                .into_iter()
                .map(|(stake_address, amount)| {
                    let bech32 = stake_address.to_string().map_err(|e| {
                        RESTError::encoding_failed(&format!("stake address: {}", e))
                    })?;

                    Ok(serde_json::json!({
                        "address": bech32,
                        "amount": amount.to_string(),
                    }))
                })
                .collect::<Result<_, RESTError>>()?;

            serialize_to_json_response(&response)
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Err(RESTError::not_found("DRep")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(_),
        )) => Err(RESTError::storage_disabled("DRep delegator")),

        _ => Err(RESTError::unexpected_response("retrieving DRep delegators")),
    }
}

pub async fn handle_drep_metadata_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = Credential::from_drep_bech32(drep_id)
        .map_err(|e| RESTError::invalid_param("drep_id", &e.to_string()))?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepMetadata {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context
        .message_bus
        .request(&handlers_config.dreps_query_topic, msg)
        .await
        .map_err(|e| RESTError::query_failed(e))?;

    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::DRepMetadata(metadata),
        )) => match metadata {
            None => {
                // metadata feature disabled
                Err(RESTError::storage_disabled("DRep metadata"))
            }
            Some(None) => {
                // enabled, but nothing stored for this DRep
                Err(RESTError::not_found("DRep metadata"))
            }
            Some(Some(anchor)) => {
                // enabled + stored → fetch the JSON
                let resp = Client::new().get(&anchor.url).send().await.map_err(|_| {
                    RESTError::InternalServerError("Failed to fetch DRep metadata URL".into())
                })?;

                let raw_bytes = resp.bytes().await.map_err(|_| {
                    RESTError::InternalServerError(
                        "Failed to read bytes from DRep metadata URL".into(),
                    )
                })?;

                let json = serde_json::from_slice::<Value>(&raw_bytes).map_err(|_| {
                    RESTError::InternalServerError("Invalid JSON from DRep metadata URL".into())
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

                serialize_to_json_response(&response)
            }
        },

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Err(RESTError::not_found("DRep metadata")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(_),
        )) => Err(RESTError::storage_disabled("DRep metadata")),

        _ => Err(RESTError::unexpected_response("retrieving DRep metadata")),
    }
}

pub async fn handle_drep_updates_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = Credential::from_drep_bech32(drep_id)
        .map_err(|e| RESTError::invalid_param("drep_id", &e.to_string()))?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepUpdates {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context
        .message_bus
        .request(&handlers_config.dreps_query_topic, msg)
        .await
        .map_err(|e| RESTError::query_failed(e))?;

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

            serialize_to_json_response(&response)
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(_),
        )) => Err(RESTError::storage_disabled("DRep updates")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Err(RESTError::not_found("DRep")),

        _ => Err(RESTError::unexpected_response("retrieving DRep updates")),
    }
}

pub async fn handle_drep_votes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = Credential::from_drep_bech32(drep_id)
        .map_err(|e| RESTError::invalid_param("drep_id", &e.to_string()))?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepVotes {
            drep_credential: credential.clone(),
        },
    )));

    let raw_msg = context
        .message_bus
        .request(&handlers_config.dreps_query_topic, msg)
        .await
        .map_err(|e| RESTError::query_failed(e))?;

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

            serialize_to_json_response(&response)
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Err(RESTError::not_found("DRep")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(_),
        )) => Err(RESTError::storage_disabled("DRep vote")),

        _ => Err(RESTError::unexpected_response("retrieving DRep votes")),
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

    let raw_msg = context
        .message_bus
        .request(&handlers_config.governance_query_topic, msg)
        .await
        .map_err(|e| RESTError::query_failed(e))?;

    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::ProposalsList(list),
        )) => {
            if list.proposals.is_empty() {
                return Ok(RESTResponse::with_json(200, "[]"));
            }

            let props_bech32: Vec<String> =
                list.proposals.iter().map(|id| id.to_bech32()).collect::<Result<_, _>>().map_err(
                    |e| RESTError::encoding_failed(&format!("proposal IDs to Bech32: {}", e)),
                )?;

            serialize_to_json_response(&props_bech32)
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(RESTError::query_failed(e)),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Err(RESTError::not_found("Proposals")),

        _ => Err(RESTError::unexpected_response("retrieving proposals list")),
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

    let raw_msg = context
        .message_bus
        .request(&handlers_config.governance_query_topic, msg)
        .await
        .map_err(|e| RESTError::query_failed(e))?;

    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    match message {
        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::ProposalInfo(info),
        )) => serialize_to_json_response(&info),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Err(RESTError::not_found("Proposal")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(RESTError::query_failed(e)),

        _ => Err(RESTError::unexpected_response("retrieving proposal info")),
    }
}

pub async fn handle_proposal_parameters_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Proposal parameters"))
}

pub async fn handle_proposal_withdrawals_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Proposal withdrawals"))
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

    let raw_msg = context
        .message_bus
        .request(&handlers_config.governance_query_topic, msg)
        .await
        .map_err(|e| RESTError::query_failed(e))?;

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

            serialize_to_json_response(&votes_list)
        }

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::NotFound,
        )) => Err(RESTError::not_found("Proposal")),

        Message::StateQueryResponse(StateQueryResponse::Governance(
            GovernanceStateQueryResponse::Error(e),
        )) => Err(RESTError::query_failed(e)),

        _ => Err(RESTError::unexpected_response("retrieving proposal votes")),
    }
}

pub async fn handle_proposal_metadata_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Proposal metadata"))
}

pub fn parse_gov_action_id(params: &[String]) -> Result<GovActionId, RESTError> {
    if params.len() != 2 {
        return Err(RESTError::BadRequest(
            "Expected two parameters: tx_hash/cert_index".into(),
        ));
    }

    let tx_hash_hex = &params[0];
    let cert_index_str = &params[1];

    let transaction_id: TxHash = hex::decode(tx_hash_hex)
        .map_err(|e| RESTError::invalid_param("tx_hash", &format!("invalid hex: {}", e)))?
        .as_slice()
        .try_into()
        .map_err(|_| RESTError::invalid_param("tx_hash", "must be 32 bytes"))?;

    let action_index = cert_index_str
        .parse::<u8>()
        .map_err(|e| RESTError::invalid_param("cert_index", &format!("expected u8: {}", e)))?;

    Ok(GovActionId {
        transaction_id,
        action_index,
    })
}
