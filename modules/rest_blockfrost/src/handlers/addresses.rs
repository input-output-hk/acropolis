use std::sync::Arc;

use crate::types::{AddressInfoExtended, AddressTotalsREST};
use crate::{handlers_config::HandlersConfig, types::AddressInfoREST};
use acropolis_common::queries::assets::{AssetsStateQuery, AssetsStateQueryResponse};
use acropolis_common::queries::errors::QueryError;
use acropolis_common::rest_error::RESTError;
use acropolis_common::AssetMetadata;
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        addresses::{AddressStateQuery, AddressStateQueryResponse},
        utils::query_state,
        utxos::{UTxOStateQuery, UTxOStateQueryResponse},
    },
    Address, Value,
};
use caryatid_sdk::Context;
use serde::Serialize;
use serde_cbor::Value as CborValue;

/// Handle `/addresses/{address}` Blockfrost-compatible endpoint
pub async fn handle_address_single_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let address = parse_address(&params)?;
    let stake_address = match address {
        Address::Shelley(ref addr) => addr.stake_address_string()?,
        _ => None,
    };

    let address_type = address.kind().to_string();
    let is_script = address.is_script();

    let address_query_msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressUTxOs {
            address: address.clone(),
        },
    )));

    let utxo_identifiers = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        address_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressUTxOs(utxo_identifiers),
            )) => Ok(Some(utxo_identifiers)),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving address UTxOs",
            )),
        },
    )
    .await?;

    let utxo_identifiers = match utxo_identifiers {
        Some(identifiers) => identifiers,
        None => {
            // Empty address - return zero balance (Blockfrost behavior)
            let rest_response = AddressInfoREST {
                address: address.to_string()?,
                amount: Value {
                    lovelace: 0,
                    assets: Vec::new(),
                }
                .into(),
                stake_address,
                address_type,
                script: is_script,
            };

            let json = serde_json::to_string_pretty(&rest_response)?;
            return Ok(RESTResponse::with_json(200, &json));
        }
    };

    let utxos_query_msg = Arc::new(Message::StateQuery(StateQuery::UTxOs(
        UTxOStateQuery::GetUTxOsSum { utxo_identifiers },
    )));

    let address_balance = query_state(
        &context,
        &handlers_config.utxos_query_topic,
        utxos_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::UTxOsSum(balance),
            )) => Ok(balance),
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving UTxO sum",
            )),
        },
    )
    .await?;

    let rest_response = AddressInfoREST {
        address: address.to_string()?,
        amount: address_balance.into(),
        stake_address,
        address_type,
        script: is_script,
    };

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/addresses/{address}/extended` Blockfrost-compatible endpoint
pub async fn handle_address_extended_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let address = parse_address(&params)?;
    let stake_address = match address {
        Address::Shelley(ref addr) => addr.stake_address_string()?,
        _ => None,
    };

    let address_type = address.kind().to_string();
    let is_script = address.is_script();

    let address_query_msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressUTxOs {
            address: address.clone(),
        },
    )));

    let utxo_identifiers = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        address_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressUTxOs(utxo_identifiers),
            )) => Ok(Some(utxo_identifiers)),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving address UTxOs",
            )),
        },
    )
    .await?;

    let utxo_identifiers = match utxo_identifiers {
        Some(identifiers) => identifiers,
        None => {
            // Empty address - return zero balance (Blockfrost behavior)
            let rest_response = AddressInfoREST {
                address: address.to_string()?,
                amount: Value {
                    lovelace: 0,
                    assets: Vec::new(),
                }
                .into(),
                stake_address,
                address_type,
                script: is_script,
            };

            let json = serde_json::to_string_pretty(&rest_response)?;
            return Ok(RESTResponse::with_json(200, &json));
        }
    };

    let utxos_query_msg = Arc::new(Message::StateQuery(StateQuery::UTxOs(
        UTxOStateQuery::GetUTxOsSum { utxo_identifiers },
    )));

    let address_balance = query_state(
        &context,
        &handlers_config.utxos_query_topic,
        utxos_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::UTxOsSum(balance),
            )) => Ok(balance),
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving UTxO sum",
            )),
        },
    )
    .await?;

    let assets_query_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetsMetadata {
            assets: address_balance.assets.clone(),
        },
    )));

    let assets_metadata = query_state(
        &context,
        &handlers_config.assets_query_topic,
        assets_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetsMetadata(balance),
            )) => Ok(balance),
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving assets metadata",
            )),
        },
    )
    .await?;

    let amount = AmountListExtended::from_value_and_metadata(address_balance, &assets_metadata);

    let rest_response = AddressInfoExtended {
        address: address.to_string()?,
        amount,
        stake_address,
        type_: address_type,
        script: is_script,
    };

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/addresses/{address}/totals` Blockfrost-compatible endpoint
pub async fn handle_address_totals_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let address = parse_address(&params)?;

    // Get totals from address state
    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressTotals {
            address: address.clone(),
        },
    )));
    let totals = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressTotals(totals),
            )) => Ok(totals),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving address totals",
            )),
        },
    )
    .await?;

    let rest_response = AddressTotalsREST {
        address: address.to_string()?,
        received_sum: totals.received.into(),
        sent_sum: totals.sent.into(),
        tx_count: totals.tx_count,
    };

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/addresses/{address}/utxos` Blockfrost-compatible endpoint
pub async fn handle_address_utxos_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Address UTxOs endpoint"))
}

/// Handle `/addresses/{address}/utxos/{asset}` Blockfrost-compatible endpoint
pub async fn handle_address_asset_utxos_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Address asset UTxOs endpoint"))
}

/// Handle `/addresses/{address}/transactions` Blockfrost-compatible endpoint
pub async fn handle_address_transactions_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Address transactions endpoint"))
}

fn parse_address(params: &[String]) -> Result<Address, RESTError> {
    let Some(address_str) = params.first() else {
        return Err(RESTError::param_missing("address"));
    };

    Ok(Address::from_string(address_str)?)
}

#[derive(Serialize)]
pub struct AmountEntryExtended {
    pub unit: String,
    pub quantity: String,
    pub decimals: Option<u64>,
    pub has_nft_onchain_metadata: bool,
}

#[derive(Serialize)]
pub struct AmountListExtended(pub Vec<AmountEntryExtended>);

impl AmountListExtended {
    pub fn from_value_and_metadata(
        value: acropolis_common::Value,
        metadata: &[AssetMetadata],
    ) -> Self {
        let mut out = Vec::new();

        out.push(AmountEntryExtended {
            unit: "lovelace".to_string(),
            quantity: value.coin().to_string(),
            decimals: Some(6),
            has_nft_onchain_metadata: false,
        });

        let mut idx = 0;

        for (policy_id, assets) in &value.assets {
            for asset in assets {
                let meta = &metadata[idx];
                idx += 1;

                // Blockfrost priority
                // 1. Set decimals to null if CIP25 metadata exists (This is an NFT)
                // 2. Set decimals based on CIP68 metadata if exists
                // 3. Set decimals based on off-chain registry
                // 4. Set decimals to null if no CIP68 metadata or off-chain registry entry

                let decimals = if meta.cip25_metadata.is_some() {
                    None
                } else if let Some(raw) = meta.cip68_metadata.as_ref() {
                    extract_cip68_decimals(raw)
                } else {
                    // TODO: off-chain registry lookup once caching exists
                    None
                };

                out.push(AmountEntryExtended {
                    unit: format!(
                        "{}{}",
                        hex::encode(policy_id),
                        hex::encode(asset.name.as_slice())
                    ),
                    quantity: asset.amount.to_string(),
                    decimals,
                    has_nft_onchain_metadata: meta.cip25_metadata.is_some(),
                });
            }
        }

        Self(out)
    }
}

pub fn extract_cip68_decimals(raw: &[u8]) -> Option<u64> {
    let decoded: CborValue = serde_cbor::from_slice(raw).ok()?;

    let arr = match decoded {
        CborValue::Array(a) => a,
        _ => return None,
    };

    if arr.len() < 2 {
        return None;
    }

    let metadata = &arr[0];

    let map = match metadata {
        CborValue::Map(m) => m,
        _ => return None,
    };

    for (key, value) in map {
        let key_str = match key {
            CborValue::Text(s) => s.as_str(),
            CborValue::Bytes(b) => std::str::from_utf8(b).ok()?,
            _ => continue,
        };

        if key_str == "decimals" {
            if let CborValue::Integer(i) = value {
                return Some(*i as u64);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{AssetName, NativeAsset, NativeAssets, Value};

    fn make_value(policy_id: [u8; 28], name: Vec<u8>, amount: u64) -> Value {
        let assets: NativeAssets = vec![(
            policy_id,
            vec![NativeAsset {
                name: AssetName::new(&name).expect("Invalid asset name"),
                amount,
            }],
        )];

        Value::new(0, assets)
    }

    fn make_metadata(cip25: Option<Vec<u8>>, cip68: Option<Vec<u8>>) -> AssetMetadata {
        AssetMetadata {
            cip25_metadata: cip25,
            cip68_metadata: cip68,
            ..Default::default()
        }
    }

    #[test]
    fn cip25_existence_overrides_decimals() {
        use serde_cbor::Value as CborValue;
        use std::collections::BTreeMap;

        let mut map = BTreeMap::new();
        map.insert(
            CborValue::Text("decimals".to_string()),
            CborValue::Integer(18),
        );

        let cbor = serde_cbor::to_vec(&vec![CborValue::Map(map), CborValue::Null]).unwrap();

        let policy_id = [1u8; 28];
        let value = make_value(policy_id, vec![0x41, 0x42], 100);

        let metadata = vec![make_metadata(Some(vec![1, 2, 3]), Some(cbor))];

        let list = AmountListExtended::from_value_and_metadata(value, &metadata);
        let asset = &list.0[1];

        // Decimals set to none when CIP25 present
        assert!(asset.decimals.is_none());

        // Onchain metadata flag is set
        assert!(asset.has_nft_onchain_metadata);
    }

    #[test]
    fn cip68_decimals_are_extracted_when_no_cip25() {
        use serde_cbor::Value as CborValue;
        use std::collections::BTreeMap;

        let mut map = BTreeMap::new();
        map.insert(CborValue::Text("decimals".into()), CborValue::Integer(18));

        let cbor = serde_cbor::to_vec(&vec![CborValue::Map(map), CborValue::Null]).unwrap();

        let policy_id = [3u8; 28];
        let value = make_value(policy_id, b"\x99".to_vec(), 999);

        let metadata = vec![make_metadata(None, Some(cbor))];

        let list = AmountListExtended::from_value_and_metadata(value, &metadata);
        let asset = &list.0[1];

        // Decimals set to value in CIP68 metadata
        assert_eq!(asset.decimals, Some(18));

        // Onchain metadata is false
        assert!(!asset.has_nft_onchain_metadata);
    }
}
