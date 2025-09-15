use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        assets::{AssetsStateQuery, AssetsStateQueryResponse},
        utils::query_state,
    },
    AssetName, PolicyId,
};
use anyhow::Result;
use caryatid_sdk::Context;
use hex::FromHex;
use std::sync::Arc;

use crate::{
    handlers_config::HandlersConfig,
    types::{MintRecordRest, PolicyAssetRest},
};

pub async fn handle_assets_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let assets_list_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetsList,
    )));

    let response = query_state(
        &context,
        &handlers_config.assets_query_topic,
        assets_list_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetsList(assets),
            )) => {
                let rest_assets: Vec<PolicyAssetRest> = assets.iter().map(Into::into).collect();
                serde_json::to_string_pretty(&rest_assets)
                    .map(|json| RESTResponse::with_json(200, &json))
                    .map_err(|e| anyhow::anyhow!("Failed to serialize assets list: {e}"))
            }
            _ => Err(anyhow::anyhow!(
                "Unexpected response while retrieving assets list",
            )),
        },
    )
    .await;

    match response {
        Ok(rest) => Ok(rest),
        Err(e) => Ok(RESTResponse::with_text(500, &format!("Query failed: {e}"))),
    }
}

pub async fn handle_asset_single_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_asset_history_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let (policy, name) = match split_policy_and_asset(&params[0]) {
        Ok(pair) => pair,
        Err(resp) => return Ok(resp),
    };

    let asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetHistory { policy, name },
    )));

    let response = match query_state(
        &context,
        &handlers_config.assets_query_topic,
        asset_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetHistory(history),
            )) => {
                let rest_history: Vec<MintRecordRest> =
                    history.iter().map(MintRecordRest::from).collect();
                match serde_json::to_string_pretty(&rest_history) {
                    Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                    Err(e) => Ok(RESTResponse::with_text(
                        500,
                        &format!("Failed to serialize asset history: {e}"),
                    )),
                }
            }
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::NotFound,
            )) => Ok(RESTResponse::with_text(404, "Asset history not found")),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(_),
            )) => Ok(RESTResponse::with_text(
                500,
                "Asset history storage is disabled in config",
            )),
            _ => Ok(RESTResponse::with_text(
                500,
                "Unexpected response while retrieving asset history",
            )),
        },
    )
    .await
    {
        Ok(rest) => rest,
        Err(e) => RESTResponse::with_text(500, &format!("Query failed: {e}")),
    };

    Ok(response)
}

pub async fn handle_asset_transactions_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_asset_addresses_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_policy_assets_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let policy: PolicyId = match <[u8; 28]>::from_hex(&params[0]) {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(RESTResponse::with_text(400, "Invalid policy_id parameter"));
        }
    };

    let asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetPolicyIdAssets { policy },
    )));

    let response = query_state(
        &context,
        &handlers_config.assets_query_topic,
        asset_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::PolicyIdAssets(assets),
            )) => {
                let rest_assets: Vec<PolicyAssetRest> = assets.iter().map(Into::into).collect();
                serde_json::to_string_pretty(&rest_assets)
                    .map(|json| RESTResponse::with_json(200, &json))
                    .map_err(|e| anyhow::anyhow!("Failed to serialize assets list: {e}"))
            }
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::NotFound,
            )) => Ok(RESTResponse::with_text(404, "Policy assets not found")),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(_),
            )) => Ok(RESTResponse::with_text(
                500,
                "Indexing by policy is disabled in config",
            )),
            _ => Ok(RESTResponse::with_text(
                500,
                "Unexpected response while retrieving policy assets",
            )),
        },
    )
    .await;

    match response {
        Ok(rest) => Ok(rest),
        Err(e) => Ok(RESTResponse::with_text(500, &format!("Query failed: {e}"))),
    }
}

fn split_policy_and_asset(hex_str: &str) -> Result<(PolicyId, AssetName), RESTResponse> {
    let decoded = match hex::decode(hex_str) {
        Ok(bytes) => bytes,
        Err(_) => return Err(RESTResponse::with_text(400, "Invalid hex string")),
    };

    if decoded.len() < 28 {
        return Err(RESTResponse::with_text(
            400,
            "Asset identifier must be at least 28 bytes",
        ));
    }

    let (policy_part, asset_part) = decoded.split_at(28);

    let policy_id: PolicyId = match policy_part.try_into() {
        Ok(arr) => arr,
        Err(_) => return Err(RESTResponse::with_text(400, "Policy id must be 28 bytes")),
    };

    let asset_name = match AssetName::new(asset_part) {
        Some(asset_name) => asset_name,
        None => {
            return Err(RESTResponse::with_text(
                400,
                "Asset name must be less than 32 bytes",
            ))
        }
    };

    Ok((policy_id, asset_name))
}
