//! REST handlers for Acropolis Blockfrost /governance endpoints
use crate::handlers_config::HandlersConfig;
use crate::types::{
    DRepInfoREST, DRepMetadataREST, DRepUpdateREST, DRepVoteREST, DRepsListREST, ProposalVoteREST,
    VoterRoleREST,
};
use acropolis_common::rest_error::RESTError;
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        errors::QueryError,
        governance::{GovernanceStateQuery, GovernanceStateQueryResponse},
        utils::query_state,
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

    let list = query_state(
        &context,
        &handlers_config.dreps_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::DRepsList(list),
            )) => Ok(list),
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

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
        .collect::<Result<Vec<_>, RESTError>>()?;

    let json = serde_json::to_string_pretty(&response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_single_drep_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = parse_drep_credential(drep_id)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepInfoWithDelegators {
            drep_credential: credential.clone(),
        },
    )));

    let response = query_state(
        &context,
        &handlers_config.dreps_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::DRepInfoWithDelegators(response),
            )) => Ok(response),
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

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
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected response from accounts-state",
            )),
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

    let json = serde_json::to_string_pretty(&response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_drep_delegators_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = parse_drep_credential(drep_id)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepDelegators {
            drep_credential: credential,
        },
    )));

    let delegators = query_state(
        &context,
        &handlers_config.dreps_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::DRepDelegators(delegators),
            )) => Ok(delegators),
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

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
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected response from accounts-state",
            )),
        },
    )
    .await?;

    let response: Vec<Value> = map
        .into_iter()
        .map(|(stake_address, amount)| {
            let bech32 = stake_address
                .to_string()
                .map_err(|e| RESTError::encoding_failed(&format!("stake address: {}", e)))?;

            Ok(serde_json::json!({
                "address": bech32,
                "amount": amount.to_string(),
            }))
        })
        .collect::<Result<Vec<_>, RESTError>>()?;

    let json = serde_json::to_string_pretty(&response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_drep_metadata_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = parse_drep_credential(drep_id)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepMetadata {
            drep_credential: credential.clone(),
        },
    )));

    let metadata = query_state(
        &context,
        &handlers_config.dreps_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::DRepMetadata(metadata),
            )) => Ok(metadata),
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

    match metadata {
        None => {
            // metadata feature disabled
            Err(RESTError::storage_disabled("DRep metadata"))
        }
        Some(None) => {
            // enabled, but nothing stored for this DRep
            Err(RESTError::not_found("DRep metadata not found"))
        }
        Some(Some(anchor)) => {
            // enabled + stored → fetch the JSON
            let resp = Client::new().get(&anchor.url).send().await.map_err(|e| {
                RESTError::query_failed(&format!("Failed to fetch DRep metadata URL: {}", e))
            })?;

            let raw_bytes = resp.bytes().await.map_err(|e| {
                RESTError::query_failed(&format!(
                    "Failed to read bytes from DRep metadata URL: {}",
                    e
                ))
            })?;

            let json: Value = serde_json::from_slice(&raw_bytes).map_err(|_| {
                RESTError::BadRequest("Invalid JSON from DRep metadata URL".to_string())
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
    }
}

pub async fn handle_drep_updates_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = parse_drep_credential(drep_id)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepUpdates {
            drep_credential: credential.clone(),
        },
    )));

    let list = query_state(
        &context,
        &handlers_config.dreps_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::DRepUpdates(list),
            )) => Ok(list),
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

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

pub async fn handle_drep_votes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let drep_id = params.first().ok_or_else(|| RESTError::param_missing("drep_id"))?;

    let credential = parse_drep_credential(drep_id)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetDRepVotes {
            drep_credential: credential.clone(),
        },
    )));

    let votes = query_state(
        &context,
        &handlers_config.dreps_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::DRepVotes(votes),
            )) => Ok(votes),
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

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

pub async fn handle_proposals_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Governance(
        GovernanceStateQuery::GetProposalsList,
    )));

    let list = query_state(
        &context,
        &handlers_config.governance_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::ProposalsList(list),
            )) => Ok(list),
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

    if list.proposals.is_empty() {
        return Ok(RESTResponse::with_json(200, "[]"));
    }

    let props_bech32: Vec<String> = list
        .proposals
        .iter()
        .map(|id| id.to_bech32())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| RESTError::encoding_failed(&format!("proposal IDs: {}", e)))?;

    let json = serde_json::to_string(&props_bech32)?;
    Ok(RESTResponse::with_json(200, &json))
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

    let info = query_state(
        &context,
        &handlers_config.governance_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::ProposalInfo(info),
            )) => Ok(info),
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

    let json = serde_json::to_string(&info)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_proposal_parameters_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Endpoint not yet implemented"))
}

pub async fn handle_proposal_withdrawals_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Endpoint not yet implemented"))
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

    let votes = query_state(
        &context,
        &handlers_config.governance_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::ProposalVotes(votes),
            )) => Ok(votes),
            Message::StateQueryResponse(StateQueryResponse::Governance(
                GovernanceStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

    let mut votes_list = Vec::new();

    for (voter, (_, voting_proc)) in votes.votes {
        let voter_role = match voter {
            Voter::ConstitutionalCommitteeKey(_) | Voter::ConstitutionalCommitteeScript(_) => {
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

pub async fn handle_proposal_metadata_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Endpoint not yet implemented"))
}

pub fn parse_gov_action_id(params: &[String]) -> Result<GovActionId, RESTError> {
    if params.len() != 2 {
        return Err(RESTError::BadRequest(
            "Expected two parameters: tx_hash/cert_index".to_string(),
        ));
    }

    let tx_hash_hex = &params[0];
    let cert_index_str = &params[1];

    let bytes = hex::decode(tx_hash_hex)
        .map_err(|e| RESTError::invalid_param("tx_hash", &format!("invalid hex: {}", e)))?;

    let transaction_id: TxHash = bytes
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

fn parse_drep_credential(drep_id: &str) -> Result<Credential, RESTError> {
    Credential::from_drep_bech32(drep_id)
        .map_err(|e| RESTError::invalid_param("drep_id", &format!("Invalid Bech32 DRep ID: {}", e)))
}
