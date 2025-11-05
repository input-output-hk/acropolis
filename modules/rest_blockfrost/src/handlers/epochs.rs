use crate::{
    handlers_config::HandlersConfig,
    types::{
        EpochActivityRest, ProtocolParamsRest, SPDDByEpochAndPoolItemRest, SPDDByEpochItemRest,
    },
};
use acropolis_common::rest_error::RESTError;
use acropolis_common::serialization::Bech32Conversion;
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        epochs::{EpochsStateQuery, EpochsStateQueryResponse},
        errors::QueryError,
        parameters::{ParametersStateQuery, ParametersStateQueryResponse},
        pools::{PoolsStateQuery, PoolsStateQueryResponse},
        spdd::{SPDDStateQuery, SPDDStateQueryResponse},
        utils::query_state,
    },
    PoolId,
};
use caryatid_sdk::Context;
use std::sync::Arc;

pub async fn handle_epoch_info_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    if params.len() != 1 {
        return Err(RESTError::BadRequest(
            "Expected one parameter: 'latest' or an epoch number".to_string(),
        ));
    }
    let param = &params[0];

    // query to get latest epoch or epoch info
    let query = if param == "latest" {
        EpochsStateQuery::GetLatestEpoch
    } else {
        let parsed = param
            .parse::<u64>()
            .map_err(|_| RESTError::invalid_param("epoch", "must be a valid number"))?;
        EpochsStateQuery::GetEpochInfo {
            epoch_number: parsed,
        }
    };

    // Get the current epoch number from epochs-state
    let epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(query)));
    let epoch_info_response = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        epoch_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(response)) => Ok(response),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving latest epoch",
            )),
        },
    )
    .await?;

    let ea_message = match epoch_info_response {
        EpochsStateQueryResponse::LatestEpoch(response) => response.epoch,
        EpochsStateQueryResponse::EpochInfo(response) => response.epoch,
        EpochsStateQueryResponse::Error(e) => return Err(e.into()),
        _ => {
            return Err(RESTError::unexpected_response(
                "Unexpected response type while retrieving epoch info",
            ))
        }
    };
    let epoch_number = ea_message.epoch;

    // For the latest epoch, query accounts-state for the stake pool delegation distribution (SPDD)
    // Otherwise, fall back to SPDD module to fetch historical epoch totals
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
                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::Error(e),
                )) => Err(e),
                _ => Err(QueryError::query_failed(
                    "Unexpected message type while retrieving the latest total active stakes",
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
                Message::StateQueryResponse(StateQueryResponse::SPDD(
                    SPDDStateQueryResponse::Error(e),
                )) => Err(e),
                _ => Err(QueryError::query_failed(&format!(
                    "Unexpected message type while retrieving total active stakes for epoch: {}",
                    epoch_number
                ))),
            },
        )
        .await?
    };

    let mut response = EpochActivityRest::from(ea_message);

    if total_active_stakes == 0 {
        response.active_stake = None;
    } else {
        response.active_stake = Some(total_active_stakes);
    }

    let json = serde_json::to_string_pretty(&response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_epoch_params_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    if params.len() != 1 {
        return Err(RESTError::BadRequest(
            "Expected one parameter: 'latest' or an epoch number".to_string(),
        ));
    }
    let param = &params[0];

    let query;
    let mut epoch_number: Option<u64> = None;

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
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving latest epoch",
            )),
        },
    )
    .await?;

    if param == "latest" {
        query = ParametersStateQuery::GetLatestEpochParameters;
    } else {
        let parsed = param
            .parse::<u64>()
            .map_err(|_| RESTError::invalid_param("epoch", "must be a valid number"))?;
        query = ParametersStateQuery::GetEpochParameters {
            epoch_number: parsed,
        };
        epoch_number = Some(parsed);
    }

    let parameters_msg = Arc::new(Message::StateQuery(StateQuery::Parameters(query)));
    let parameters_response = query_state(
        &context,
        &handlers_config.parameters_query_topic,
        parameters_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Parameters(resp)) => Ok(resp),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving parameters",
            )),
        },
    )
    .await?;

    match parameters_response {
        ParametersStateQueryResponse::LatestEpochParameters(params) => {
            let rest = ProtocolParamsRest::from((latest_epoch, params));
            let json = serde_json::to_string_pretty(&rest)?;
            Ok(RESTResponse::with_json(200, &json))
        }
        ParametersStateQueryResponse::EpochParameters(params) => {
            let epoch = epoch_number.expect("epoch_number must exist for EpochParameters");

            if epoch > latest_epoch {
                return Err(RESTError::not_found(
                    "Protocol parameters not found for requested epoch",
                ));
            }
            let rest = ProtocolParamsRest::from((epoch, params));
            let json = serde_json::to_string_pretty(&rest)?;
            Ok(RESTResponse::with_json(200, &json))
        }
        ParametersStateQueryResponse::Error(e) => Err(e.into()),
        _ => Err(RESTError::unexpected_response(
            "Unexpected response type while retrieving parameters",
        )),
    }
}

pub async fn handle_epoch_next_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    if params.len() != 1 {
        return Err(RESTError::BadRequest(
            "Expected one parameter: an epoch number".to_string(),
        ));
    }
    let param = &params[0];

    let parsed = param
        .parse::<u64>()
        .map_err(|_| RESTError::invalid_param("epoch", "must be a valid number"))?;

    let next_epochs_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetNextEpochs {
            epoch_number: parsed,
        },
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
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving next epochs",
            )),
        },
    )
    .await?;

    let json = serde_json::to_string_pretty(&next_epochs)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_epoch_previous_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    if params.len() != 1 {
        return Err(RESTError::BadRequest(
            "Expected one parameter: an epoch number".to_string(),
        ));
    }
    let param = &params[0];

    let parsed = param
        .parse::<u64>()
        .map_err(|_| RESTError::invalid_param("epoch", "must be a valid number"))?;

    let previous_epochs_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetPreviousEpochs {
            epoch_number: parsed,
        },
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
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving previous epochs",
            )),
        },
    )
    .await?;

    let json = serde_json::to_string_pretty(&previous_epochs)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_epoch_total_stakes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    if params.len() != 1 {
        return Err(RESTError::BadRequest(
            "Expected one parameter: an epoch number".to_string(),
        ));
    }
    let param = &params[0];

    let epoch_number = param
        .parse::<u64>()
        .map_err(|_| RESTError::invalid_param("epoch", "must be a valid number"))?;

    // Query latest epoch from epochs-state
    let latest_epoch_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        latest_epoch_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving latest epoch",
            )),
        },
    )
    .await?;

    if epoch_number > latest_epoch {
        return Err(RESTError::not_found("Epoch not found"));
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
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving SPDD by epoch",
            )),
        },
    )
    .await?;

    let spdd_response: Vec<SPDDByEpochItemRest> = spdd
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
        .collect::<Result<Vec<_>, RESTError>>()?;

    let json = serde_json::to_string_pretty(&spdd_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_epoch_pool_stakes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    if params.len() != 2 {
        return Err(RESTError::BadRequest(
            "Expected two parameters: an epoch number and a pool ID".to_string(),
        ));
    }
    let param = &params[0];
    let pool_id = &params[1];

    let epoch_number = param
        .parse::<u64>()
        .map_err(|_| RESTError::invalid_param("epoch", "must be a valid number"))?;

    let pool_id = PoolId::from_bech32(pool_id).map_err(|_| {
        RESTError::invalid_param(
            "pool_id",
            &format!("Invalid Bech32 stake pool ID: {}", pool_id),
        )
    })?;

    // Query latest epoch from epochs-state
    let latest_epoch_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        latest_epoch_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving latest epoch",
            )),
        },
    )
    .await?;

    if epoch_number > latest_epoch {
        return Err(RESTError::not_found("Epoch not found"));
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
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving SPDD by epoch and pool",
            )),
        },
    )
    .await?;

    let spdd_response: Vec<SPDDByEpochAndPoolItemRest> = spdd
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
        .collect::<Result<Vec<_>, RESTError>>()?;

    let json = serde_json::to_string_pretty(&spdd_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_epoch_total_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Endpoint not yet implemented"))
}

pub async fn handle_epoch_pool_blocks_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    if params.len() != 2 {
        return Err(RESTError::BadRequest(
            "Expected two parameters: an epoch number and a pool ID".to_string(),
        ));
    }
    let epoch_number_param = &params[0];
    let pool_id_param = &params[1];

    let epoch_number = epoch_number_param
        .parse::<u64>()
        .map_err(|_| RESTError::invalid_param("epoch", "must be a valid number"))?;

    let spo = PoolId::from_bech32(pool_id_param).map_err(|_| {
        RESTError::invalid_param(
            "pool_id",
            &format!("Invalid Bech32 stake pool ID: {}", pool_id_param),
        )
    })?;

    // query Pool's Blocks by epoch from spo-state
    let msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetBlocksByPoolAndEpoch {
            pool_id: spo,
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
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving pool block hashes by epoch",
            )),
        },
    )
    .await?;

    // NOTE:
    // Need to query chain_store
    // to get block_hash for each block height
    let json = serde_json::to_string_pretty(&blocks)?;
    Ok(RESTResponse::with_json(200, &json))
}
