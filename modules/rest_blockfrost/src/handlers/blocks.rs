//! REST handlers for Acropolis Blockfrost /blocks endpoints
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        blocks::{BlocksStateQuery, BlocksStateQueryResponse},
        utils::query_state,
    },
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::sync::Arc;

use crate::handlers_config::HandlersConfig;
use crate::types::BlockInfoREST;

/// Handle `/blocks/latest`
pub async fn handle_blocks_latest_blockfrost(
    context: Arc<Context<Message>>,
    _: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let blocks_latest_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
                BlocksStateQuery::GetLatestBlock,
    )));
    let block_info = query_state(
        &context,
        &handlers_config.blocks_query_topic,
        blocks_latest_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::LatestBlock(blocks_latest),
            )) => Ok(blocks_latest),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving latest block: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving latest block"
                ))
            }
        },
    )
    .await?;

    match serde_json::to_string(&BlockInfoREST(&block_info)) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving block info: {e}"),
        )),
    }
}
