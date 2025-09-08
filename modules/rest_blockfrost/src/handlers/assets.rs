use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        assets::{AssetsStateQuery, AssetsStateQueryResponse},
        utils::query_state,
    },
    serialization::Bech32WithHrp,
    AssetName, PolicyId,
};
use anyhow::Result;
use blake2::Blake2b;
use caryatid_sdk::Context;
use digest::{consts::U20, Digest};
use hex::FromHex;
use reqwest::Client;
use std::sync::Arc;

use crate::{
    handlers_config::HandlersConfig,
    types::{AssetInfoRest, AssetListEntryRest, AssetMetadata, MintRecordRest},
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
            )) => Ok(assets),
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected response while retrieving assets list"
                ))
            }
        },
    )
    .await?;

    let rest: Vec<AssetListEntryRest> = response
        .into_iter()
        .map(|(policy_id, name, amount)| AssetListEntryRest {
            asset: format!("{}{}", hex::encode(policy_id), hex::encode(name)),
            quantity: amount.to_string(),
        })
        .collect();

    match serde_json::to_string_pretty(&rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Failed to serialize assets list: {e}"),
        )),
    }
}

pub async fn handle_policy_assets_asset_single_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = &params[0];
    let asset_query_msg;

    if param == "policy" {
        // Policy-level query
        let policy_id: PolicyId = match <[u8; 28]>::from_hex(&params[1]) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(RESTResponse::with_text(400, "Invalid policy_id parameter"));
            }
        };

        asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
            AssetsStateQuery::GetPolicyIdAssets { policy_id },
        )));

        // Run the query, no fingerprint/metadata involved
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
                _ => Ok(RESTResponse::with_text(
                    500,
                    "Unexpected response while retrieving policy assets",
                )),
            },
        )
        .await;

        return match response {
            Ok(rest) => Ok(rest),
            Err(e) => Ok(RESTResponse::with_text(500, &format!("Query failed: {e}"))),
        };
    } else {
        // Single-asset query
        let (policy_id, asset_name) = match split_policy_and_asset(&params[0]) {
            Ok(pair) => pair,
            Err(resp) => return Ok(resp),
        };

        asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
            AssetsStateQuery::GetAssetInfo {
                policy_id,
                asset_name,
            },
        )));

        let asset = params[0].clone();
        let (pid_str, an_str) = asset.split_at(56);

        // Precompute fingerprint
        let bytes = hex::decode(&asset).expect("invalid asset hex");
        let mut hasher = Blake2b::<U20>::new();
        hasher.update(&bytes);
        let hash: Vec<u8> = hasher.finalize().to_vec();
        let fingerprint = hash.to_bech32_with_hrp("asset").expect("bech32 encoding failed");

        // Fetch registry metadata
        let off_chain_metadata = fetch_asset_metadata(&asset).await;

        // Prepare clones for closure
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
                    let response = AssetInfoRest {
                        asset: asset_for_closure.clone(),
                        policy_id: pid_for_closure.clone(),
                        asset_name: an_for_closure.clone(),
                        fingerprint: fingerprint_for_closure.clone(),
                        quantity: quantity.to_string(),
                        initial_mint_tx_hash: hex::encode(info.initial_mint_tx_hash),
                        mint_or_burn_count: info.mint_or_burn_count,
                        onchain_metadata: info.onchain_metadata,
                        onchain_metadata_standard: info.metadata_standard,
                        onchain_metadata_extra: info.metadata_extra.map(hex::encode),
                        metadata: metadata_for_closure.clone(),
                    };

                    serde_json::to_string_pretty(&response)
                        .map(|json| RESTResponse::with_json(200, &json))
                        .map_err(|e| anyhow::anyhow!("Failed to serialize asset info: {e}"))
                }
                Message::StateQueryResponse(StateQueryResponse::Assets(
                    AssetsStateQueryResponse::NotFound,
                )) => Ok(RESTResponse::with_text(500, "Asset not found")),
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
}

pub async fn handle_asset_history_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let (policy_id, asset_name) = match split_policy_and_asset(&params[0]) {
        Ok(pair) => pair,
        Err(resp) => return Ok(resp),
    };

    let asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetHistory {
            policy_id,
            asset_name,
        },
    )));

    let response = match query_state(
        &context,
        &handlers_config.assets_query_topic,
        asset_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetHistory(history),
            )) => {
                let rest: Vec<MintRecordRest> = history.into_iter().map(Into::into).collect();
                match serde_json::to_string_pretty(&rest) {
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

pub fn split_policy_and_asset(hex_str: &str) -> Result<(PolicyId, AssetName), RESTResponse> {
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

    let asset_name: AssetName = asset_part.to_vec();

    Ok((policy_id, asset_name))
}

#[derive(Debug, serde::Deserialize)]
struct RegistryEntry {
    name: Option<RegistryField<String>>,
    description: Option<RegistryField<String>>,
    ticker: Option<RegistryField<String>>,
    url: Option<RegistryField<String>>,
    logo: Option<RegistryField<String>>,
    decimals: Option<RegistryField<u8>>,
}
#[derive(Debug, serde::Deserialize)]
struct RegistryField<T> {
    value: T,
    #[serde(flatten)]
    _ignore: serde_json::Value,
}

async fn fetch_asset_metadata(asset: &str) -> Option<AssetMetadata> {
    let base_url =  "https://raw.githubusercontent.com/input-output-hk/metadata-registry-testnet/master/mappings/";
    let url = format!("{base_url}{asset}.json");
    let client = Client::new();
    let resp = match client.get(&url).send().await {
        Ok(resp) => resp,
        Err(_) => return None,
    };
    let entry: RegistryEntry = match resp.json().await {
        Ok(entry) => entry,
        Err(_) => return None,
    };
    match (entry.name, entry.description) {
        (Some(n), Some(d)) => Some(AssetMetadata {
            name: n.value,
            description: d.value,
            ticker: entry.ticker.map(|t| t.value),
            url: entry.url.map(|u| u.value),
            logo: entry.logo.map(|l| l.value),
            decimals: entry.decimals.map(|d| d.value),
        }),
        _ => None,
    }
}
