//! REST handlers for Acropolis Blockfrost /blocks endpoints
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        blocks::{BlockKey, BlocksStateQuery, BlocksStateQueryResponse},
        utils::query_state,
    },
    BlockHash,
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::sync::Arc;

use crate::handlers_config::HandlersConfig;
use crate::types::BlockInfoREST;

fn parse_block_key(key: &str) -> Result<BlockKey> {
    match key.len() {
        64 => match hex::decode(key) {
            Ok(key) => Ok(BlockKey::Hash(BlockHash::from(key))),
            Err(error) => Err(error.into()),
        },
        _ => match key.parse::<u64>() {
            Ok(key) => Ok(BlockKey::Number(key)),
            Err(error) => Err(error.into()),
        },
    }
}

/// Handle `/blocks/latest`, `/blocks/{hash_or_number}`
pub async fn handle_blocks_latest_hash_number_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    match param.as_str() {
        "latest" => handle_blocks_latest_blockfrost(context, handlers_config).await,
        _ => handle_blocks_hash_or_number_blockfrost(context, param, handlers_config).await,
    }
}

/// Handle `/blocks/latest`
async fn handle_blocks_latest_blockfrost(
    context: Arc<Context<Message>>,
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

/// Handle `/blocks/{hash_or_number}`
async fn handle_blocks_hash_or_number_blockfrost(
    context: Arc<Context<Message>>,
    hash_or_number: &str,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let block_key = match parse_block_key(hash_or_number) {
        Ok(block_key) => block_key,
        Err(error) => return Err(error),
    };

    let block_info_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetBlockInfo { block_key },
    )));
    let block_info = query_state(
        &context,
        &handlers_config.blocks_query_topic,
        block_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockInfo(block_info),
            )) => Ok(Some(block_info)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving block by hash or number: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving block by hash or number"
                ))
            }
        },
    )
    .await?;

    match block_info {
        Some(block_info) => match serde_json::to_string(&BlockInfoREST(&block_info)) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving block info: {e}"),
            )),
        },
        None => Ok(RESTResponse::with_text(404, "Not found")),
    }
}

/// Handle `/blocks/slot/{slot_number}`
pub async fn handle_blocks_slot_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let slot = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    let slot = match slot.parse::<u64>() {
        Ok(slot) => slot,
        Err(error) => return Err(error.into()),
    };

    let block_slot_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetBlockBySlot { slot },
    )));
    let block_info = query_state(
        &context,
        &handlers_config.blocks_query_topic,
        block_slot_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockBySlot(block_info),
            )) => Ok(Some(block_info)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving block by slot: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving block by slot"
                ))
            }
        },
    )
    .await?;

    match block_info {
        Some(block_info) => match serde_json::to_string(&BlockInfoREST(&block_info)) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving block info: {e}"),
            )),
        },
        None => Ok(RESTResponse::with_text(404, "Not found")),
    }
}

/// Handle `/blocks/epoch/{epoch_number}/slot/{slot_number}`
pub async fn handle_blocks_epoch_slot_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let (epoch, slot) = match params.as_slice() {
        [param1, param2] => (param1, param2),
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    let epoch = match epoch.parse::<u64>() {
        Ok(epoch) => epoch,
        Err(error) => return Err(error.into()),
    };

    let slot = match slot.parse::<u64>() {
        Ok(slot) => slot,
        Err(error) => return Err(error.into()),
    };

    let block_epoch_slot_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetBlockByEpochSlot { epoch, slot },
    )));
    let block_info = query_state(
        &context,
        &handlers_config.blocks_query_topic,
        block_epoch_slot_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockByEpochSlot(block_info),
            )) => Ok(Some(block_info)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving block by epoch slot: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving block by epoch slot"
                ))
            }
        },
    )
    .await?;

    match block_info {
        Some(block_info) => match serde_json::to_string(&BlockInfoREST(&block_info)) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving block info: {e}"),
            )),
        },
        None => Ok(RESTResponse::with_text(404, "Not found")),
    }
}
