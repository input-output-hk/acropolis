//! REST handlers for Acropolis Blockfrost /epochs endpoints
use crate::{
    handlers_config::HandlersConfig,
    types::{
        EpochActivityRest, ProtocolParamsRest, SPDDByEpochAndPoolItemRest, SPDDByEpochItemRest,
    },
};
use acropolis_common::app_error::RESTError;
use acropolis_common::serialization::Bech32Conversion;
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        epochs::{EpochsStateQuery, EpochsStateQueryResponse},
        parameters::{ParametersStateQuery, ParametersStateQueryResponse},
        pools::{PoolsStateQuery, PoolsStateQueryResponse},
        spdd::{SPDDStateQuery, SPDDStateQueryResponse},
        utils::{query_state, serialize_to_json_response},
    },
    PoolId,
};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Handle `/epochs/latest` and `/epochs/{number}`
pub async fn handle_epoch_info_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let param =
        params.first().ok_or_else(|| RESTError::invalid_param("epoch", "parameter is missing"))?;

    // Query to get latest epoch or specific epoch info
    let query = if param == "latest" {
        EpochsStateQuery::GetLatestEpoch
    } else {
        let epoch_number = param.parse::<u64>()?;
        EpochsStateQuery::GetEpochInfo { epoch_number }
    };

    let epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(query)));
    let epoch_info_response = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        epoch_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(response)) => Ok(response),
            _ => Err(RESTError::unexpected_response("retrieving epoch info")),
        },
    )
    .await?;

    let ea_message = match epoch_info_response {
        EpochsStateQueryResponse::LatestEpoch(response) => Ok(response.epoch),
        EpochsStateQueryResponse::EpochInfo(response) => Ok(response.epoch),
        EpochsStateQueryResponse::NotFound => Err(RESTError::not_found("Epoch")),
        EpochsStateQueryResponse::Error(e) => Err(RESTError::InternalServerError(format!(
            "Error retrieving epoch info: {}",
            e
        ))),
        _ => Err(RESTError::unexpected_response("retrieving epoch info")),
    }?;

    let epoch_number = ea_message.epoch;

    // For the latest epoch, query accounts-state for active stakes
    // Otherwise, fall back to SPDD module for historical data
    let total_active_stakes: u64 = if param == "latest" {
        let total_active_stakes_msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
            AccountsStateQuery::GetActiveStakes {},
        )));
        query_state(
            &context,
            &handlers_config.accounts_query_topic,
            total_active_stakes_msg,
            |message| match message {
                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::ActiveStakes(total_active_stake),
                )) => Ok(total_active_stake),
                _ => Err(RESTError::unexpected_response(
                    "retrieving total active stakes",
                )),
            },
        )
        .await?
    } else {
        // Historical epoch: use SPDD if available
        let total_active_stakes_msg = Arc::new(Message::StateQuery(StateQuery::SPDD(
            SPDDStateQuery::GetEpochTotalActiveStakes {
                epoch: epoch_number,
            },
        )));
        query_state(
            &context,
            &handlers_config.spdd_query_topic,
            total_active_stakes_msg,
            |message| match message {
                Message::StateQueryResponse(StateQueryResponse::SPDD(
                    SPDDStateQueryResponse::EpochTotalActiveStakes(total_active_stakes),
                )) => Ok(total_active_stakes),
                _ => Err(RESTError::unexpected_response(&format!(
                    "retrieving total active stakes for epoch {}",
                    epoch_number
                ))),
            },
        )
        .await?
    };

    let mut response = EpochActivityRest::from(ea_message);
    response.active_stake = if total_active_stakes == 0 {
        None
    } else {
        Some(total_active_stakes)
    };

    serialize_to_json_response(&response)
}

/// Handle `/epochs/latest/parameters` and `/epochs/{number}/parameters`
pub async fn handle_epoch_params_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let param =
        params.first().ok_or_else(|| RESTError::invalid_param("epoch", "parameter is missing"))?;

    // Get current epoch number from epochs-state
    let latest_epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        latest_epoch_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving latest epoch: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response("retrieving latest epoch")),
        },
    )
    .await?;

    let (query, epoch_number) = if param == "latest" {
        (ParametersStateQuery::GetLatestEpochParameters, None)
    } else {
        let parsed = param.parse::<u64>()?;
        (
            ParametersStateQuery::GetEpochParameters {
                epoch_number: parsed,
            },
            Some(parsed),
        )
    };

    let parameters_msg = Arc::new(Message::StateQuery(StateQuery::Parameters(query)));
    let parameters_response = query_state(
        &context,
        &handlers_config.parameters_query_topic,
        parameters_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Parameters(resp)) => Ok(resp),
            _ => Err(RESTError::unexpected_response("retrieving parameters")),
        },
    )
    .await?;

    match parameters_response {
        ParametersStateQueryResponse::LatestEpochParameters(params) => {
            let rest = ProtocolParamsRest::from((latest_epoch, params));
            serialize_to_json_response(&rest)
        }
        ParametersStateQueryResponse::EpochParameters(params) => {
            let epoch = epoch_number.expect("epoch_number must exist for EpochParameters");

            if epoch > latest_epoch {
                return Err(RESTError::not_found(
                    "Protocol parameters for requested epoch",
                ));
            }

            let rest = ProtocolParamsRest::from((epoch, params));
            serialize_to_json_response(&rest)
        }
        ParametersStateQueryResponse::NotFound => Err(RESTError::not_found(
            "Protocol parameters for requested epoch",
        )),
        ParametersStateQueryResponse::Error(msg) => Err(RESTError::BadRequest(msg)),
        _ => Err(RESTError::unexpected_response("retrieving parameters")),
    }
}

/// Handle `/epochs/{number}/next`
pub async fn handle_epoch_next_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let param =
        params.first().ok_or_else(|| RESTError::invalid_param("epoch", "parameter is missing"))?;

    let epoch_number = param.parse::<u64>()?;

    let next_epochs_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetNextEpochs { epoch_number },
    )));

    let next_epochs = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        next_epochs_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::NextEpochs(response),
            )) => Ok(response.epochs.into_iter().map(EpochActivityRest::from).collect::<Vec<_>>()),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving next epochs: {}",
                e
            ))),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::NotFound,
            )) => Err(RESTError::not_found("Epoch")),
            _ => Err(RESTError::unexpected_response("retrieving next epochs")),
        },
    )
    .await?;

    serialize_to_json_response(&next_epochs)
}

/// Handle `/epochs/{number}/previous`
pub async fn handle_epoch_previous_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let param =
        params.first().ok_or_else(|| RESTError::invalid_param("epoch", "parameter is missing"))?;

    let epoch_number = param.parse::<u64>()?;

    let previous_epochs_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetPreviousEpochs { epoch_number },
    )));

    let previous_epochs = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        previous_epochs_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::PreviousEpochs(response),
            )) => Ok(response.epochs.into_iter().map(EpochActivityRest::from).collect::<Vec<_>>()),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving previous epochs: {}",
                e
            ))),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::NotFound,
            )) => Err(RESTError::not_found("Epoch")),
            _ => Err(RESTError::unexpected_response("retrieving previous epochs")),
        },
    )
    .await?;

    serialize_to_json_response(&previous_epochs)
}

/// Handle `/epochs/{number}/stakes`
pub async fn handle_epoch_total_stakes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let param =
        params.first().ok_or_else(|| RESTError::invalid_param("epoch", "parameter is missing"))?;

    let epoch_number = param.parse::<u64>()?;

    // Query latest epoch from epochs-state
    let latest_epoch = fetch_latest_epoch(&context, &handlers_config).await?;

    if epoch_number > latest_epoch {
        return Err(RESTError::not_found("Epoch"));
    }

    // Query SPDD by epoch from accounts-state
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetSPDDByEpoch {
            epoch: epoch_number,
        },
    )));

    let spdd = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::SPDDByEpoch(res),
            )) => Ok(res),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving SPDD by epoch: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response("retrieving SPDD by epoch")),
        },
    )
    .await?;

    let spdd_response: Result<Vec<_>, RESTError> = spdd
        .into_iter()
        .map(|(pool_id, stake_address, amount)| {
            let bech32 = stake_address
                .to_string()
                .map_err(|e| RESTError::encoding_failed(&format!("stake address: {}", e)))?;
            Ok(SPDDByEpochItemRest {
                pool_id,
                stake_address: bech32,
                amount,
            })
        })
        .collect();

    serialize_to_json_response(&spdd_response?)
}

/// Handle `/epochs/{number}/stakes/{pool_id}`
pub async fn handle_epoch_pool_stakes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let (epoch_param, pool_id_param) = match params.as_slice() {
        [e, p] => (e, p),
        _ => {
            return Err(RESTError::invalid_param(
                "parameters",
                "epoch number and pool ID required",
            ))
        }
    };

    let epoch_number = epoch_param.parse::<u64>()?;

    let pool_id = PoolId::from_bech32(pool_id_param)
        .map_err(|_| RESTError::invalid_param("pool_id", "invalid Bech32 stake pool ID"))?;

    // Query latest epoch from epochs-state
    let latest_epoch = fetch_latest_epoch(&context, &handlers_config).await?;

    if epoch_number > latest_epoch {
        return Err(RESTError::not_found("Epoch"));
    }

    // Query SPDD by epoch and pool from accounts-state
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetSPDDByEpochAndPool {
            epoch: epoch_number,
            pool_id,
        },
    )));

    let spdd = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::SPDDByEpochAndPool(res),
            )) => Ok(res),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving SPDD by epoch and pool: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response(
                "retrieving SPDD by epoch and pool",
            )),
        },
    )
    .await?;

    let spdd_response: Result<Vec<_>, RESTError> = spdd
        .into_iter()
        .map(|(stake_address, amount)| {
            let bech32 = stake_address
                .to_string()
                .map_err(|e| RESTError::encoding_failed(&format!("stake address: {}", e)))?;
            Ok(SPDDByEpochAndPoolItemRest {
                stake_address: bech32,
                amount,
            })
        })
        .collect();

    serialize_to_json_response(&spdd_response?)
}

/// Handle `/epochs/{number}/blocks` - Not implemented
pub async fn handle_epoch_total_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Epoch total blocks"))
}

/// Handle `/epochs/{number}/blocks/{pool_id}`
pub async fn handle_epoch_pool_blocks_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let (epoch_param, pool_id_param) = match params.as_slice() {
        [e, p] => (e, p),
        _ => {
            return Err(RESTError::invalid_param(
                "parameters",
                "epoch number and pool ID required",
            ))
        }
    };

    let epoch_number = epoch_param.parse::<u64>()?;

    let pool_id = PoolId::from_bech32(pool_id_param)
        .map_err(|_| RESTError::invalid_param("pool_id", "invalid Bech32 stake pool ID"))?;

    // Query pool's blocks by epoch from pools-state
    let msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetBlocksByPoolAndEpoch {
            pool_id,
            epoch: epoch_number,
        },
    )));

    let blocks = query_state(
        &context,
        &handlers_config.pools_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::BlocksByPoolAndEpoch(blocks),
            )) => Ok(blocks),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving pool block hashes by epoch: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response(
                "retrieving pool block hashes by epoch",
            )),
        },
    )
    .await?;

    // NOTE: Need to query chain_store to get block_hash for each block height
    serialize_to_json_response(&blocks)
}

/// Fetch the latest epoch number
async fn fetch_latest_epoch(
    context: &Arc<Context<Message>>,
    handlers_config: &HandlersConfig,
) -> Result<u64, RESTError> {
    let latest_epoch_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));

    query_state(
        context,
        &handlers_config.epochs_query_topic,
        latest_epoch_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch.epoch),
            _ => Err(RESTError::unexpected_response("retrieving latest epoch")),
        },
    )
    .await
}
