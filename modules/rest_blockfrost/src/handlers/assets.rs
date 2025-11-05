use crate::{
    handlers_config::HandlersConfig,
    types::{
        AssetAddressRest, AssetInfoRest, AssetMetadata, AssetMintRecordRest, AssetTransactionRest,
        PolicyAssetRest,
    },
};
use acropolis_common::queries::assets::{AssetsStateQuery, AssetsStateQueryResponse};
use acropolis_common::rest_error::RESTError;
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{errors::QueryError, utils::query_state},
    serialization::Bech32WithHrp,
    AssetMetadataStandard, AssetName, PolicyId,
};
use blake2::{digest::consts::U20, Blake2b, Digest};
use caryatid_sdk::Context;
use hex::FromHex;
use reqwest::Client;
use serde_cbor::Value as CborValue;
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_assets_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let assets_list_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetsList,
    )));

    let assets = query_state(
        &context,
        &handlers_config.assets_query_topic,
        assets_list_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetsList(assets),
            )) => Ok(assets),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected response while retrieving asset list",
            )),
        },
    )
    .await?;

    let rest_assets: Vec<PolicyAssetRest> = assets.iter().map(Into::into).collect();
    let json = serde_json::to_string_pretty(&rest_assets)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_asset_single_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let asset = params[0].clone();
    let (policy, name) = split_policy_and_asset(&asset)?;

    let asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetInfo { policy, name },
    )));

    let (policy_str, name_str) = asset.split_at(56);

    let bytes = hex::decode(&asset).map_err(|_| RESTError::invalid_hex())?;

    let mut hasher = Blake2b::<U20>::new();
    hasher.update(&bytes);
    let hash: Vec<u8> = hasher.finalize().to_vec();
    let fingerprint = hash
        .to_bech32_with_hrp("asset")
        .map_err(|e| RESTError::encoding_failed(&format!("asset fingerprint: {}", e)))?;

    let off_chain_metadata =
        fetch_asset_metadata(&asset, &handlers_config.offchain_token_registry_url).await;

    let policy_id = policy_str.to_string();
    let asset_name = name_str.to_string();

    let (quantity, info) = query_state(
        &context,
        &handlers_config.assets_query_topic,
        asset_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetInfo(data),
            )) => Ok(data),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected response while retrieving asset info",
            )),
        },
    )
    .await?;

    let (onchain_metadata_json, onchain_metadata_extra, cip68_version) = info
        .onchain_metadata
        .as_ref()
        .map(|raw_meta| normalize_onchain_metadata(raw_meta.as_slice()))
        .unwrap_or((None, None, None));

    let onchain_metadata_standard = cip68_version.or(info.metadata_standard);

    // TODO: Query transaction_state once implemented to fetch inital_mint_tx_hash based on TxIdentifier
    let response = AssetInfoRest {
        asset,
        policy_id,
        asset_name,
        fingerprint,
        quantity: quantity.to_string(),
        initial_mint_tx_hash: "transaction_state not yet implemented".to_string(),
        mint_or_burn_count: info.mint_or_burn_count,
        onchain_metadata: onchain_metadata_json,
        onchain_metadata_standard,
        onchain_metadata_extra,
        metadata: off_chain_metadata,
    };

    let json = serde_json::to_string_pretty(&response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_asset_history_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let (policy, name) = split_policy_and_asset(&params[0])?;

    let asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetHistory { policy, name },
    )));

    let history = query_state(
        &context,
        &handlers_config.assets_query_topic,
        asset_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetHistory(history),
            )) => Ok(history),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected response while retrieving asset history",
            )),
        },
    )
    .await?;

    let rest_history: Vec<AssetMintRecordRest> = history.iter().map(Into::into).collect();
    let json = serde_json::to_string_pretty(&rest_history)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_asset_transactions_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let (policy, name) = split_policy_and_asset(&params[0])?;

    let asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetTransactions { policy, name },
    )));

    let txs = query_state(
        &context,
        &handlers_config.assets_query_topic,
        asset_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetTransactions(txs),
            )) => Ok(txs),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected response while retrieving asset transactions",
            )),
        },
    )
    .await?;

    // TODO: Query transaction_state once implemented to fetch tx_hash and block_time using TxIdentifier
    let rest_txs: Vec<AssetTransactionRest> = txs
        .iter()
        .map(|identifier| AssetTransactionRest {
            tx_hash: "transaction_state not yet implemented".to_string(),
            tx_index: identifier.tx_index(),
            block_height: identifier.block_number(),
            block_time: "transaction_state not yet implemented".to_string(),
        })
        .collect();

    let json = serde_json::to_string_pretty(&rest_txs)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_asset_addresses_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let (policy, name) = split_policy_and_asset(&params[0])?;

    let asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetAddresses { policy, name },
    )));

    let addresses = query_state(
        &context,
        &handlers_config.assets_query_topic,
        asset_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetAddresses(addresses),
            )) => Ok(addresses),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected response while retrieving asset addresses",
            )),
        },
    )
    .await?;

    let rest_addrs: Vec<AssetAddressRest> =
        addresses.iter().map(AssetAddressRest::try_from).collect::<Result<Vec<_>, _>>().map_err(
            |e| RESTError::InternalServerError(format!("Failed to convert address entry: {}", e)),
        )?;

    let json = serde_json::to_string_pretty(&rest_addrs)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_policy_assets_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let policy: PolicyId = <[u8; 28]>::from_hex(&params[0])
        .map_err(|_| RESTError::invalid_param("policy_id", "invalid hex format"))?;

    let asset_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetPolicyIdAssets { policy },
    )));

    let assets = query_state(
        &context,
        &handlers_config.assets_query_topic,
        asset_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::PolicyIdAssets(assets),
            )) => Ok(assets),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected response while retrieving policy assets",
            )),
        },
    )
    .await?;

    let rest_assets: Vec<PolicyAssetRest> = assets.iter().map(Into::into).collect();
    let json = serde_json::to_string_pretty(&rest_assets)?;
    Ok(RESTResponse::with_json(200, &json))
}

fn split_policy_and_asset(hex_str: &str) -> Result<(PolicyId, AssetName), RESTError> {
    let decoded = hex::decode(hex_str).map_err(|_| RESTError::invalid_hex())?;

    if decoded.len() < 28 {
        return Err(RESTError::BadRequest(
            "Asset identifier must be at least 28 bytes".to_string(),
        ));
    }

    let (policy_part, asset_part) = decoded.split_at(28);

    let policy_id: PolicyId = policy_part
        .try_into()
        .map_err(|_| RESTError::BadRequest("Policy id must be 28 bytes".to_string()))?;

    let asset_name = AssetName::new(asset_part).ok_or_else(|| {
        RESTError::BadRequest("Asset name must be less than 32 bytes".to_string())
    })?;

    Ok((policy_id, asset_name))
}

pub async fn fetch_asset_metadata(
    asset: &str,
    offchain_registry_url: &str,
) -> Option<AssetMetadata> {
    let url = format!("{}{}.json", offchain_registry_url, asset);

    let client = Client::new();
    let res = client.get(&url).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }

    let raw: Value = res.json().await.ok()?;

    // Name and description are required
    let get_str = |key: &str| {
        raw.get(key).and_then(|f| f.get("value")).and_then(|v| v.as_str()).map(|s| s.to_string())
    };
    let name = get_str("name")?;
    let description = get_str("description")?;

    // Remaining fields are optional
    let ticker = get_str("ticker");
    let url = get_str("url");
    let logo = get_str("logo");
    let decimals = raw
        .get("decimals")
        .and_then(|f| f.get("value"))
        .and_then(|v| v.as_u64())
        .and_then(|n| u8::try_from(n).ok());

    Some(AssetMetadata {
        name,
        description,
        ticker,
        url,
        logo,
        decimals,
    })
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
                CborValue::Integer(1) => Some(AssetMetadataStandard::CIP68v1),
                CborValue::Integer(2) => Some(AssetMetadataStandard::CIP68v2),
                CborValue::Integer(3) => Some(AssetMetadataStandard::CIP68v3),
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

            let extra_hex = serde_cbor::to_vec(&extra)
                .ok()
                .map(hex::encode)
                .filter(|val| !matches!(val.as_str(), "80" | "f6"));
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

// NOTE: Blockfrost preserves the on-chain field order for `onchain_metadata`.
//       This REST handler serializes with `serde`, which produces fields in lexicographical order.
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
        CborValue::Bytes(b) => match String::from_utf8(b) {
            Ok(s) => Value::String(s),
            Err(b) => Value::String(hex::encode(b.into_bytes())),
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

#[cfg(test)]
mod tests {
    use crate::handlers::assets::split_policy_and_asset;
    use hex;

    fn policy_bytes() -> [u8; 28] {
        [0u8; 28]
    }

    #[test]
    fn invalid_hex_string() {
        let result = split_policy_and_asset("zzzz");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert_eq!(err.message(), "Invalid hex string");
    }

    #[test]
    fn too_short_input() {
        let hex_str = hex::encode([1u8, 2, 3]);
        let result = split_policy_and_asset(&hex_str);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert_eq!(err.message(), "Asset identifier must be at least 28 bytes");
    }

    #[test]
    fn invalid_asset_name_too_long() {
        let mut bytes = policy_bytes().to_vec();
        bytes.extend(vec![0u8; 33]);
        let hex_str = hex::encode(bytes);
        let result = split_policy_and_asset(&hex_str);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert_eq!(err.message(), "Asset name must be less than 32 bytes");
    }

    #[test]
    fn valid_policy_and_asset() {
        let mut bytes = policy_bytes().to_vec();
        bytes.extend_from_slice(b"MyToken");
        let hex_str = hex::encode(bytes);
        let result = split_policy_and_asset(&hex_str);
        assert!(result.is_ok());
        let (policy, name) = result.unwrap();
        assert_eq!(policy, policy_bytes());
        assert_eq!(name.as_slice(), b"MyToken");
    }
}
