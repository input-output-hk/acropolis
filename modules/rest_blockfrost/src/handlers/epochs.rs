use crate::{
    handlers_config::HandlersConfig,
    types::{
        EpochActivityRest, ProtocolParamsRest, SPDDByEpochAndPoolItemRest, SPDDByEpochItemRest,
    },
};
use acropolis_common::queries::errors::QueryError;
use acropolis_common::rest_error::RESTError;
use acropolis_common::serialization::Bech32Conversion;
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        epochs::{EpochsStateQuery, EpochsStateQueryResponse},
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
            )) => Ok(res.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving latest epoch",
            )),
        },
    )
    .await?;

    let (is_latest, mut response) = if param == "latest" {
        (true, EpochActivityRest::from(latest_epoch))
    } else {
        let parsed = param
            .parse::<u64>()
            .map_err(|_| RESTError::invalid_param("epoch", "invalid epoch number"))?;

        if parsed > latest_epoch.epoch {
            return Err(RESTError::not_found("Epoch not found"));
        }

        if parsed == latest_epoch.epoch {
            (true, EpochActivityRest::from(latest_epoch))
        } else {
            let epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
                EpochsStateQuery::GetEpochInfo {
                    epoch_number: parsed,
                },
            )));
            let epoch_info = query_state(
                &context,
                &handlers_config.historical_epochs_query_topic,
                epoch_info_msg,
                |message| match message {
                    Message::StateQueryResponse(StateQueryResponse::Epochs(
                        EpochsStateQueryResponse::EpochInfo(response),
                    )) => Ok(EpochActivityRest::from(response.epoch)),
                    Message::StateQueryResponse(StateQueryResponse::Epochs(
                        EpochsStateQueryResponse::Error(QueryError::NotFound { .. }),
                    )) => Err(QueryError::not_found("Epoch not found")),
                    Message::StateQueryResponse(StateQueryResponse::Epochs(
                        EpochsStateQueryResponse::Error(e),
                    )) => Err(e),
                    _ => Err(QueryError::internal_error(
                        "Unexpected message type while retrieving epoch info",
                    )),
                },
            )
            .await?;
            (false, epoch_info)
        }
    };

    // For the latest epoch, query accounts-state for the stake pool delegation distribution (SPDD)
    // Otherwise, fall back to SPDD module to fetch historical epoch totals
    // if spdd_storage is not enabled, return NULL for active_stakes
    let epoch_number = response.epoch;
    let total_active_stakes = if is_latest {
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
                )) => Ok(Some(total_active_stake)),
                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::Error(_),
                )) => Ok(None),
                _ => Err(QueryError::internal_error(
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
                                            )) => Ok(Some(total_active_stakes)),
                Message::StateQueryResponse(StateQueryResponse::SPDD(
                                                SPDDStateQueryResponse::Error(_),
                                            )) => Ok(None),
                _ => Err(QueryError::internal_error(
                    format!("Unexpected message type while retrieving total active stakes for epoch: {epoch_number}"),
                )),
            },
        )
            .await?
    }.unwrap_or(0);

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
            _ => Err(QueryError::internal_error(
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
            .map_err(|_| RESTError::invalid_param("epoch", "invalid epoch number"))?;
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
            _ => Err(QueryError::internal_error(
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
        ParametersStateQueryResponse::Error(QueryError::NotFound { .. }) => Err(
            RESTError::not_found("Protocol parameters not found for requested epoch"),
        ),
        ParametersStateQueryResponse::Error(e) => Err(e.into()),
        _ => Err(RESTError::unexpected_response(
            "Unexpected message type while retrieving parameters",
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
        .map_err(|_| RESTError::invalid_param("epoch", "invalid epoch number"))?;

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
            )) => Ok(res.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving latest epoch",
            )),
        },
    )
    .await?;

    if parsed > latest_epoch.epoch {
        return Err(RESTError::not_found(
            format!("Epoch {parsed} not found").as_str(),
        ));
    }

    if parsed == latest_epoch.epoch {
        return Ok(RESTResponse::with_json(200, "[]"));
    }

    let next_epochs_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetNextEpochs {
            epoch_number: parsed,
        },
    )));

    let mut next_epochs = query_state(
        &context,
        &handlers_config.historical_epochs_query_topic,
        next_epochs_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::NextEpochs(response),
            )) => Ok(response.epochs.into_iter().map(EpochActivityRest::from).collect::<Vec<_>>()),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving next epochs",
            )),
        },
    )
    .await?;
    next_epochs.push(EpochActivityRest::from(latest_epoch));

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
        .map_err(|_| RESTError::invalid_param("epoch", "invalid epoch number"))?;

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
            )) => Ok(res.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving latest epoch",
            )),
        },
    )
    .await?;

    if parsed > latest_epoch.epoch {
        return Err(RESTError::not_found(
            format!("Epoch {parsed} not found").as_str(),
        ));
    }

    let previous_epochs_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetPreviousEpochs {
            epoch_number: parsed,
        },
    )));
    let previous_epochs = query_state(
        &context,
        &handlers_config.historical_epochs_query_topic,
        previous_epochs_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::PreviousEpochs(response),
            )) => Ok(response.epochs.into_iter().map(EpochActivityRest::from).collect::<Vec<_>>()),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
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
        .map_err(|_| RESTError::invalid_param("epoch", "invalid epoch number"))?;

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
            _ => Err(QueryError::internal_error(
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
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving SPDD by epoch",
            )),
        },
    )
    .await?;

    let spdd_response = spdd
        .into_iter()
        .map(|(pool_id, stake_address, amount)| {
            let bech32 = stake_address.to_string().map_err(|e| {
                RESTError::InternalServerError(format!(
                    "Failed to convert stake address to string: {}",
                    e
                ))
            })?;
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
    let pool_id_str = &params[1];

    let epoch_number = param
        .parse::<u64>()
        .map_err(|_| RESTError::invalid_param("epoch", "invalid epoch number"))?;

    let pool_id = PoolId::from_bech32(pool_id_str)
        .map_err(|_| RESTError::invalid_param("pool_id", "invalid Bech32 stake pool ID"))?;

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
            _ => Err(QueryError::internal_error(
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
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving SPDD by epoch and pool",
            )),
        },
    )
    .await?;

    let spdd_response = spdd
        .into_iter()
        .map(|(stake_address, amount)| {
            let bech32 = stake_address.to_string().map_err(|e| {
                RESTError::InternalServerError(format!(
                    "Failed to convert stake address to string: {}",
                    e
                ))
            })?;
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
    Err(RESTError::not_implemented("Epoch total blocks endpoint"))
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
        .map_err(|_| RESTError::invalid_param("epoch", "invalid epoch number"))?;

    let spo = PoolId::from_bech32(pool_id_param)
        .map_err(|_| RESTError::invalid_param("pool_id", "invalid Bech32 stake pool ID"))?;

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
            _ => Err(QueryError::internal_error("Unexpected message type")),
        },
    )
    .await?;

    // NOTE:
    // Need to query chain_store
    // to get block_hash for each block height

    let json = serde_json::to_string_pretty(&blocks)?;
    Ok(RESTResponse::with_json(200, &json))
}
