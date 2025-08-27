use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        epochs::{EpochsStateQuery, EpochsStateQueryResponse, DEFAULT_PARAMETERS_QUERY_TOPIC},
        get_query_topic,
    },
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::sync::Arc;
use tracing::info;

use crate::types::ProtocolParamsRest;

pub async fn handle_epoch_info_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_params_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
) -> Result<RESTResponse> {
    info!("Inside handle epoch params");
    if params.len() != 1 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected one parameter: 'latest' or an epoch number",
        ));
    }
    let param = &params[0];

    let query = if param == "latest" {
        EpochsStateQuery::GetLatestEpochParameters
    } else {
        match param.parse::<u64>() {
            Ok(epoch_number) => EpochsStateQuery::GetEpochParameters { epoch_number },
            Err(_) => {
                return Ok(RESTResponse::with_text(
                    400,
                    "Invalid epoch number parameter",
                ))
            }
        }
    };
    info!("Query: {:?}", query);

    let msg = Arc::new(Message::StateQuery(StateQuery::Epochs(query)));
    let parameters_query_topic = get_query_topic(context.clone(), DEFAULT_PARAMETERS_QUERY_TOPIC);
    let raw_msg = context.message_bus.request(&parameters_query_topic, msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());
    info!("Message: {:?}", message);
    match message {
        Message::StateQueryResponse(StateQueryResponse::Epochs(resp)) => {
            info!("EpochsStateQueryResponse received: {:?}", resp);
            match resp {
                EpochsStateQueryResponse::LatestEpochParameters(params) => {
                    let rest = ProtocolParamsRest::from((1, params));
                    match serde_json::to_string_pretty(&rest) {
                        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                        Err(e) => Ok(RESTResponse::with_text(
                            500,
                            &format!("Failed to serialize parameters: {e}"),
                        )),
                    }
                }
                EpochsStateQueryResponse::EpochParameters(params) => {
                    let rest = ProtocolParamsRest::from((1, params));
                    match serde_json::to_string_pretty(&rest) {
                        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                        Err(e) => Ok(RESTResponse::with_text(
                            500,
                            &format!("Failed to serialize parameters: {e}"),
                        )),
                    }
                }
                EpochsStateQueryResponse::NotFound => Ok(RESTResponse::with_text(
                    404,
                    "Protocol parameters not found for requested epoch",
                )),
                EpochsStateQueryResponse::Error(msg) => Ok(RESTResponse::with_text(400, &msg)),
                _ => Ok(RESTResponse::with_text(
                    500,
                    "Unexpected EpochsStateQueryResponse message",
                )),
            }
        }
        _ => {
            return Ok(RESTResponse::with_text(
                500,
                "Unexpected StateQueryResponse message",
            ));
        }
    }
}

pub async fn handle_epoch_next_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_previous_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_total_stakes_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_pool_stakes_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_total_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_pool_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}
