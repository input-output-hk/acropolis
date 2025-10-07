use crate::{
    handlers_config::HandlersConfig,
    types::{EpochActivityRest, ProtocolParamsRest},
};
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        epochs::{EpochsStateQuery, EpochsStateQueryResponse},
        parameters::{ParametersStateQuery, ParametersStateQueryResponse}
        ,
        spdd::{SPDDStateQuery, SPDDStateQueryResponse},
        utils::query_state,
    },
};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;
use std::sync::Arc;

pub async fn handle_epoch_info_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    if params.len() != 1 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected one parameter: 'latest' or an epoch number",
        ));
    }
    let param = &params[0];
    let query;

    // query to get latest epoch or epoch info
    if param == "latest" {
        query = EpochsStateQuery::GetLatestEpoch;
    } else {
        let parsed = match param.parse::<u64>() {
            Ok(num) => num,
            Err(_) => {
                return Ok(RESTResponse::with_text(
                    400,
                    "Invalid epoch number parameter",
                ));
            }
        };
        query = EpochsStateQuery::GetEpochInfo {
            epoch_number: parsed,
        };
    }

    // Get the current epoch number from epochs-state
    let epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(query)));
    let epoch_info_response = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        epoch_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(response)) => Ok(response),
            _ => {
                return Err(anyhow!(
                    "Unexpected message type while retrieving latest epoch"
                ))
            }
        },
    )
    .await?;

    let ea_message = match epoch_info_response {
        EpochsStateQueryResponse::LatestEpoch(response) => Ok(response.epoch),
        EpochsStateQueryResponse::EpochInfo(response) => Ok(response.epoch),
        EpochsStateQueryResponse::NotFound => Err(anyhow!("Epoch not found")),
        EpochsStateQueryResponse::Error(e) => Err(anyhow!(
            "Internal server error while retrieving epoch info: {e}"
        )),
        _ => Err(anyhow!(
            "Unexpected message type while retrieving epoch info"
        )),
    }?;
    let epoch_number = ea_message.epoch;

    // For the latest epoch, query accounts-state for the stake pool delegation distribution (SPDD)
    // Otherwise, fall back to SPDD module to fetch historical epoch totals
    let total_active_stakes: Option<u64> = if param == "latest" {
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
                _ => Err(anyhow::anyhow!(
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
                    SPDDStateQueryResponse::Error(_e),
                )) => Ok(None),
                _ => Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving total active stakes for epoch: {epoch_number}",
                )),
            },
        )
        .await?
    };

    let mut response = EpochActivityRest::from(ea_message);
    response.active_stake = total_active_stakes;
    let json = match serde_json::to_string(&response) {
        Ok(j) => j,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving latest epoch: {e}"),
            ));
        }
    };
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_epoch_params_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    if params.len() != 1 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected one parameter: 'latest' or an epoch number",
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
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving latest epoch: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving latest epoch"
                ))
            }
        },
    )
    .await?;

    if param == "latest" {
        query = ParametersStateQuery::GetLatestEpochParameters;
    } else {
        let parsed = match param.parse::<u64>() {
            Ok(num) => num,
            Err(_) => {
                return Ok(RESTResponse::with_text(
                    400,
                    "Invalid epoch number parameter",
                ));
            }
        };
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
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving parameters"
            )),
        },
    )
    .await?;

    match parameters_response {
        ParametersStateQueryResponse::LatestEpochParameters(params) => {
            let rest = ProtocolParamsRest::from((latest_epoch, params));
            match serde_json::to_string_pretty(&rest) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to serialize parameters: {e}"),
                )),
            }
        }
        ParametersStateQueryResponse::EpochParameters(params) => {
            let epoch = epoch_number.expect("epoch_number must exist for EpochParameters");

            if epoch > latest_epoch {
                return Ok(RESTResponse::with_text(
                    404,
                    "Protocol parameters not found for requested epoch",
                ));
            }
            let rest = ProtocolParamsRest::from((epoch, params));
            match serde_json::to_string_pretty(&rest) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to serialize parameters: {e}"),
                )),
            }
        }
        ParametersStateQueryResponse::NotFound => Ok(RESTResponse::with_text(
            404,
            "Protocol parameters not found for requested epoch",
        )),
        ParametersStateQueryResponse::Error(msg) => Ok(RESTResponse::with_text(400, &msg)),
    }
}

pub async fn handle_epoch_next_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_previous_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_total_stakes_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_pool_stakes_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_total_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_pool_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}
