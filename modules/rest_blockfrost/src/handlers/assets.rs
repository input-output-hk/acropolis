use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        assets::{AssetMetadataStandard, AssetsStateQuery, AssetsStateQueryResponse},
        utils::query_state,
    },
    serialization::Bech32WithHrp,
    AssetName, PolicyId,
};
use anyhow::Result;
use blake2::{digest::consts::U20, Blake2b, Digest};
use caryatid_sdk::Context;
use hex::FromHex;
use reqwest::Client;
use serde_cbor::Value as CborValue;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::info;

use crate::{handlers_config::HandlersConfig, types::AssetInfoRest};

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
            )) => Ok(assets),
            _ => Err(anyhow::anyhow!(
                "Unexpected response while retrieving assets list",
            )),
        },
    )
    .await?;

    match serde_json::to_string_pretty(&response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Failed to serialize assets list: {e}"),
        )),
    }
}

pub async fn handle_asset_single_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let (policy, name) = match split_policy_and_asset(&params[0]) {
        Ok(pair) => pair,
        Err(resp) => return Ok(resp),
    };

    let asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetInfo { policy, name },
    )));

    let asset = params[0].clone();
    let (pid_str, an_str) = asset.split_at(56);

    let bytes = hex::decode(&asset).expect("invalid asset hex");
    let mut hasher = Blake2b::<U20>::new();
    hasher.update(&bytes);
    let hash: Vec<u8> = hasher.finalize().to_vec();
    let fingerprint = hash.to_bech32_with_hrp("asset").expect("bech32 encoding failed");
    let off_chain_metadata = fetch_asset_metadata(&asset).await;

    let asset_for_closure = asset.clone();
    let pid_for_closure = pid_str.to_string();
    let an_for_closure = an_str.to_string();
    let fingerprint_for_closure = fingerprint.clone();
    let metadata_for_closure = off_chain_metadata.clone();

    let response = query_state(
        &context,
        &handlers_config.assets_query_topic,
        asset_query_msg,
        move |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetInfo((quantity, info)),
            )) => {
                if let Some(raw) = info.onchain_metadata.as_ref() {
                    info!(
                        "Raw onchain_metadata (len={}): {}",
                        raw.len(),
                        hex::encode(raw.as_slice())
                    );
                } else {
                    info!("Raw onchain_metadata: None");
                }

                let (onchain_metadata_json, onchain_metadata_extra, cip68_version) = info
                    .onchain_metadata
                    .as_ref()
                    .map(|arc| normalize_onchain_metadata(arc.as_slice()))
                    .unwrap_or((None, None, None));

                let onchain_metadata_standard = cip68_version.or(info.metadata_standard);

                let response = AssetInfoRest {
                    asset: asset_for_closure.clone(),
                    policy_id: pid_for_closure.clone(),
                    asset_name: an_for_closure.clone(),
                    fingerprint: fingerprint_for_closure.clone(),
                    quantity: quantity.to_string(),
                    initial_mint_tx_hash: hex::encode(info.initial_mint_tx_hash),
                    mint_or_burn_count: info.mint_or_burn_count,
                    onchain_metadata: onchain_metadata_json,
                    onchain_metadata_standard,
                    onchain_metadata_extra,
                    metadata: metadata_for_closure.clone(),
                };

                serde_json::to_string_pretty(&response)
                    .map(|json| RESTResponse::with_json(200, &json))
                    .map_err(|e| anyhow::anyhow!("Failed to serialize asset info: {e}"))
            }
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::NotFound,
            )) => Ok(RESTResponse::with_text(404, "Asset not found")),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(_),
            )) => Ok(RESTResponse::with_text(
                500,
                "Asset info storage disabled in config",
            )),
            _ => Ok(RESTResponse::with_text(
                500,
                "Unexpected response while retrieving asset info",
            )),
        },
    )
    .await;

    return match response {
        Ok(rest) => Ok(rest),
        Err(e) => Ok(RESTResponse::with_text(500, &format!("Query failed: {e}"))),
    };
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
            )) => match serde_json::to_string_pretty(&history) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to serialize asset history: {e}"),
                )),
            },
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
            )) => serde_json::to_string_pretty(&assets)
                .map(|json| RESTResponse::with_json(200, &json))
                .map_err(|e| anyhow::anyhow!("Failed to serialize assets list: {e}")),
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

pub async fn fetch_asset_metadata(asset: &str) -> Option<Value> {
    let url = format!(
        "https://raw.githubusercontent.com/cardano-foundation/cardano-token-registry/master/mappings/{}.json",
        asset
    );

    let client = Client::new();
    let res = client.get(&url).send().await.ok()?;

    if !res.status().is_success() {
        return None;
    }

    let raw: Value = res.json().await.ok()?;

    let output = json!({
        "name": raw.get("name").and_then(|f| f.get("value")),
        "ticker": raw.get("ticker").and_then(|f| f.get("value")),
        "decimals": raw.get("decimals").and_then(|f| f.get("value")),
        "description": raw.get("description").and_then(|f| f.get("value")),
        "logo": raw.get("logo").and_then(|f| f.get("value")),
        "url": raw.get("url").and_then(|f| f.get("value")),
    });

    Some(output)
}

/// Normalize on-chain metadata for CIP-25 and CIP-68.
/// Returns (metadata_json, metadata_extra, cip68_version).
pub fn normalize_onchain_metadata(
    raw: &[u8],
) -> (Option<Value>, Option<String>, Option<AssetMetadataStandard>) {
    let decoded: CborValue = match serde_cbor::from_slice(raw) {
        Ok(val) => val,
        Err(_) => return (None, None, None),
    };

    match decoded {
        CborValue::Tag(_, boxed) => {
            normalize_onchain_metadata(&serde_cbor::to_vec(&*boxed).unwrap_or_default())
        }

        // CIP-68
        CborValue::Array(mut arr) if arr.len() >= 2 => {
            let metadata = arr.remove(0);
            let version = match arr.remove(0) {
                CborValue::Integer(i) => match i {
                    1 => Some(AssetMetadataStandard::CIP68v1),
                    2 => Some(AssetMetadataStandard::CIP68v2),
                    3 => Some(AssetMetadataStandard::CIP68v3),
                    _ => Some(AssetMetadataStandard::CIP68v1),
                },
                _ => Some(AssetMetadataStandard::CIP68v1),
            };
            let extra = arr.pop().unwrap_or(CborValue::Array(vec![]));

            let json_meta = match metadata {
                CborValue::Map(map) => {
                    let mut obj = serde_json::Map::new();
                    for (k, v) in map {
                        let key_str = match k {
                            CborValue::Bytes(b) => {
                                String::from_utf8(b.clone()).unwrap_or_else(|_| hex::encode(b))
                            }
                            CborValue::Text(t) => t,
                            _ => continue,
                        };
                        obj.insert(key_str, cbor_to_json(v));
                    }
                    Some(Value::Object(obj))
                }
                _ => None,
            };

            let extra_hex = serde_cbor::to_vec(&extra).ok().map(hex::encode);
            (json_meta, extra_hex, version)
        }

        // CIP-25: plain map
        CborValue::Map(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                if let CborValue::Text(key) = k {
                    obj.insert(key, cbor_to_json(v));
                }
            }
            (Some(Value::Object(obj)), None, None)
        }

        _ => (None, None, None),
    }
}

// TODO: Order fields deterministically to align with blockfrost
fn cbor_to_json(val: CborValue) -> Value {
    match val {
        CborValue::Text(s) => Value::String(s),
        CborValue::Integer(i) => {
            if let Some(n) = serde_json::Number::from_i128(i) {
                Value::Number(n)
            } else {
                Value::String(i.to_string())
            }
        }
        CborValue::Bytes(b) => match String::from_utf8(b.clone()) {
            Ok(s) => Value::String(s),
            Err(_) => Value::String(hex::encode(b)),
        },
        CborValue::Array(arr) => Value::Array(arr.into_iter().map(cbor_to_json).collect()),
        CborValue::Map(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                if let CborValue::Text(key) = k {
                    obj.insert(key, cbor_to_json(v));
                }
            }
            Value::Object(obj)
        }
        _ => Value::Null,
    }
}
