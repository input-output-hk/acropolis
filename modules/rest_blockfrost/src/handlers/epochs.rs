use std::sync::Arc;

use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        epochs::{EpochsStateQuery, EpochsStateQueryResponse},
        utils::query_state,
    },
};
use anyhow::Result;
use caryatid_sdk::Context;

use crate::{handlers_config::HandlersConfig, types::EpochActivityRest};

pub async fn handle_latest_epoch_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    // prepare message
    let msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));

    let ea_message = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving latest epoch: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    let response = EpochActivityRest::from(ea_message);
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

pub async fn handle_single_epoch_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    let Ok(epoch) = param.parse::<u64>() else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid epoch number: {param}"),
        ));
    };

    // prepare message
    let msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetEpochInfo {
            epoch_number: epoch,
        },
    )));

    let ea_message = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::EpochInfo(res),
            )) => Ok(res.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving epoch info of {epoch}: {e}"
            )),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!("Epoch not found")),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    let response = EpochActivityRest::from(ea_message);
    let json = match serde_json::to_string(&response) {
        Ok(j) => j,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving epoch info of {epoch}: {e}"),
            ));
        }
    };
    Ok(RESTResponse::with_json(200, &json))
}
