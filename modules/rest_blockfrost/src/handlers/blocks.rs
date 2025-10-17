//! REST handlers for Acropolis Blockfrost /blocks endpoints
use acropolis_common::{
    extract_strict_query_params,
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        blocks::{BlockKey, BlocksStateQuery, BlocksStateQueryResponse},
        misc::Order,
        utils::rest_query_state,
    },
    BlockHash,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;
use std::collections::HashMap;
use std::sync::Arc;

use crate::handlers_config::HandlersConfig;
use crate::types::BlockInfoREST;

fn parse_block_key(key: &str) -> Result<BlockKey> {
    match key.len() {
        64 => match hex::decode(key) {
            Ok(key) => match BlockHash::try_from(key) {
                Ok(block_hash) => Ok(BlockKey::Hash(block_hash)),
                Err(_) => Err(anyhow::Error::msg("Invalid block hash")),
            },
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
        _ => handle_blocks_hash_number_blockfrost(context, param, handlers_config).await,
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
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        blocks_latest_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::LatestBlock(blocks_latest),
            )) => Some(Ok(Some(BlockInfoREST(blocks_latest)))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

/// Handle `/blocks/{hash_or_number}`
async fn handle_blocks_hash_number_blockfrost(
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
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        block_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockInfo(block_info),
            )) => Some(Ok(Some(BlockInfoREST(block_info)))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Some(Ok(None)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

/// Handle `/blocks/latest/txs`, `/blocks/{hash_or_number}/txs`
pub async fn handle_blocks_latest_hash_number_transactions_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    query_params: HashMap<String, String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    extract_strict_query_params!(query_params, {
        "count" => limit: Option<u64>,
        "page" => page: Option<u64>,
        "order" => order: Option<Order>,
    });
    let limit = limit.unwrap_or(100);
    let skip = (page.unwrap_or(1) - 1) * limit;
    let order = order.unwrap_or(Order::Asc);

    match param.as_str() {
        "latest" => {
            handle_blocks_latest_transactions_blockfrost(
                context,
                limit,
                skip,
                order,
                handlers_config,
            )
            .await
        }
        _ => {
            handle_blocks_hash_number_transactions_blockfrost(
                context,
                param,
                limit,
                skip,
                order,
                handlers_config,
            )
            .await
        }
    }
}

/// Handle `/blocks/latest/txs`
async fn handle_blocks_latest_transactions_blockfrost(
    context: Arc<Context<Message>>,
    limit: u64,
    skip: u64,
    order: Order,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let blocks_latest_txs_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetLatestBlockTransactions { limit, skip, order },
    )));
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        blocks_latest_txs_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::LatestBlockTransactions(blocks_txs),
            )) => Some(Ok(Some(blocks_txs))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

/// Handle `/blocks/{hash_or_number}/txs`
async fn handle_blocks_hash_number_transactions_blockfrost(
    context: Arc<Context<Message>>,
    hash_or_number: &str,
    limit: u64,
    skip: u64,
    order: Order,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let block_key = match parse_block_key(hash_or_number) {
        Ok(block_key) => block_key,
        Err(error) => return Err(error),
    };

    let block_txs_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetBlockTransactions {
            block_key,
            limit,
            skip,
            order,
        },
    )));
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        block_txs_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockTransactions(block_txs),
            )) => Some(Ok(Some(block_txs))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Some(Ok(None)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

/// Handle `/blocks/latest/txs/cbor`, `/blocks/{hash_or_number}/txs/cbor`
pub async fn handle_blocks_latest_hash_number_transactions_cbor_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    query_params: HashMap<String, String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    extract_strict_query_params!(query_params, {
        "count" => limit: Option<u64>,
        "page" => page: Option<u64>,
        "order" => order: Option<Order>,
    });
    let limit = limit.unwrap_or(100);
    let skip = (page.unwrap_or(1) - 1) * limit;
    let order = order.unwrap_or(Order::Asc);

    match param.as_str() {
        "latest" => {
            handle_blocks_latest_transactions_cbor_blockfrost(
                context,
                limit,
                skip,
                order,
                handlers_config,
            )
            .await
        }
        _ => {
            handle_blocks_hash_number_transactions_cbor_blockfrost(
                context,
                param,
                limit,
                skip,
                order,
                handlers_config,
            )
            .await
        }
    }
}

/// Handle `/blocks/latest/txs/cbor`
async fn handle_blocks_latest_transactions_cbor_blockfrost(
    context: Arc<Context<Message>>,
    limit: u64,
    skip: u64,
    order: Order,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let blocks_latest_txs_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetLatestBlockTransactionsCBOR { limit, skip, order },
    )));
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        blocks_latest_txs_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::LatestBlockTransactionsCBOR(blocks_txs),
            )) => Some(Ok(Some(blocks_txs))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

/// Handle `/blocks/{hash_or_number}/txs/cbor`
async fn handle_blocks_hash_number_transactions_cbor_blockfrost(
    context: Arc<Context<Message>>,
    hash_or_number: &str,
    limit: u64,
    skip: u64,
    order: Order,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let block_key = match parse_block_key(hash_or_number) {
        Ok(block_key) => block_key,
        Err(error) => return Err(error),
    };

    let block_txs_cbor_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetBlockTransactionsCBOR {
            block_key,
            limit,
            skip,
            order,
        },
    )));
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        block_txs_cbor_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockTransactionsCBOR(block_txs_cbor),
            )) => Some(Ok(Some(block_txs_cbor))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Some(Ok(None)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

/// Handle `/blocks/{hash_or_number}/next`
pub async fn handle_blocks_hash_number_next_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    query_params: HashMap<String, String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    let block_key = match parse_block_key(param) {
        Ok(block_key) => block_key,
        Err(error) => return Err(error),
    };

    extract_strict_query_params!(query_params, {
        "count" => limit: Option<u64>,
        "page" => page: Option<u64>,
    });
    let limit = limit.unwrap_or(100);
    let skip = (page.unwrap_or(1) - 1) * limit;

    let blocks_next_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetNextBlocks {
            block_key,
            limit,
            skip,
        },
    )));
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        blocks_next_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NextBlocks(blocks_next),
            )) => Some(Ok(Some(blocks_next))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Some(Ok(None)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

/// Handle `/blocks/{hash_or_number}/previous`
pub async fn handle_blocks_hash_number_previous_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    query_params: HashMap<String, String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    let block_key = match parse_block_key(param) {
        Ok(block_key) => block_key,
        Err(error) => return Err(error),
    };

    extract_strict_query_params!(query_params, {
        "count" => limit: Option<u64>,
        "page" => page: Option<u64>,
    });
    let limit = limit.unwrap_or(100);
    let skip = (page.unwrap_or(1) - 1) * limit;

    let blocks_previous_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetPreviousBlocks {
            block_key,
            limit,
            skip,
        },
    )));
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        blocks_previous_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::PreviousBlocks(blocks_previous),
            )) => Some(Ok(Some(blocks_previous))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Some(Ok(None)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
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
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        block_slot_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockBySlot(block_info),
            )) => Some(Ok(Some(block_info))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Some(Ok(None)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
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
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        block_epoch_slot_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockByEpochSlot(block_info),
            )) => Some(Ok(Some(block_info))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Some(Ok(None)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

/// Handle `/blocks/{hash_or_number}/addresses`
pub async fn handle_blocks_hash_number_addresses_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    query_params: HashMap<String, String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    let block_key = match parse_block_key(param) {
        Ok(block_key) => block_key,
        Err(error) => return Err(error),
    };

    extract_strict_query_params!(query_params, {
        "count" => limit: Option<u64>,
        "page" => page: Option<u64>,
    });
    let limit = limit.unwrap_or(100);
    let skip = (page.unwrap_or(1) - 1) * limit;

    let block_involved_addresses_msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetBlockInvolvedAddresses {
            block_key,
            limit,
            skip,
        },
    )));
    rest_query_state(
        &context,
        &handlers_config.blocks_query_topic,
        block_involved_addresses_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockInvolvedAddresses(block_addresses),
            )) => Some(Ok(Some(block_addresses))),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::NotFound,
            )) => Some(Ok(None)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}
