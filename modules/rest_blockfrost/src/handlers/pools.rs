//! REST handlers for Acropolis Blockfrost /pools endpoints
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::pools::{PoolsStateQuery, PoolsStateQueryResponse},
    serialization::Bech32WithHrp,
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::sync::Arc;

/// Handle `/pools` Blockfrost-compatible endpoint
pub async fn handle_pools_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsList,
    )));

    // Send message via message bus
    let raw = context.message_bus.request("pools-state", msg).await?;

    // Unwrap and match
    let message = Arc::try_unwrap(raw).unwrap_or_else(|arc| (*arc).clone());

    let pool_operators = match message {
        Message::StateQueryResponse(StateQueryResponse::Pools(
            PoolsStateQueryResponse::PoolsList(pools),
        )) => pools.pool_operators,

        Message::StateQueryResponse(StateQueryResponse::Pools(PoolsStateQueryResponse::Error(
            e,
        ))) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving pools list: {e}"),
            ));
        }

        _ => return Ok(RESTResponse::with_text(500, "Unexpected message type")),
    };

    let pool_ids = pool_operators
        .iter()
        .map(|operator| operator.to_bech32_with_hrp("pool"))
        .collect::<Result<Vec<String>, _>>();

    match pool_ids {
        Ok(pool_ids) => match serde_json::to_string(&pool_ids) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving pools list: {e}"),
            )),
        },
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pools list: {e}"),
        )),
    }
}

pub async fn handle_pools_extended_retired_retiring_single_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_history_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_metadata_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_relays_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_delegators_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_updates_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_votes_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}
