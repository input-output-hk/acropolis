use anyhow::Result;
use std::sync::Arc;

use crate::{handlers_config::HandlersConfig, types::AddressInfoREST};
use acropolis_common::queries::errors::QueryError;
use acropolis_common::rest_error::RESTError;
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

/// Handle `/addresses/{address}` Blockfrost-compatible endpoint
pub async fn handle_address_single_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let [address_str] = &params[..] else {
        return Ok(RESTResponse::with_text(400, "Missing address parameter"));
    };

    let (address, stake_address) = match Address::from_string(address_str) {
        Ok(Address::None) | Ok(Address::Stake(_)) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid address '{address_str}'"),
            ));
        }
        Ok(Address::Byron(byron)) => (Address::Byron(byron), None),
        Ok(Address::Shelley(shelley)) => {
            let stake_addr = match shelley.stake_address_string() {
                Ok(stake_addr) => stake_addr,
                Err(e) => {
                    return Ok(RESTResponse::with_text(
                        400,
                        &format!("Invalid address '{address_str}': {e}"),
                    ));
                }
            };

            (Address::Shelley(shelley), stake_addr)
        }
        Err(e) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid address '{}': {e}", params[0]),
            ));
        }
    };

    let address_type = address.kind().to_string();
    let is_script = address.is_script();

    let address_query_msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressUTxOs { address },
    )));

    let utxo_query_result = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        address_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressUTxOs(utxo_identifiers),
            )) => Ok(Some(utxo_identifiers)),

            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected response from addresses query",
            )),
        },
    )
    .await;

    let utxo_identifiers = match utxo_query_result {
        Ok(Some(utxo_identifiers)) => utxo_identifiers,
        Ok(None) => {
            let rest_response = AddressInfoREST {
                address: address_str.to_string(),
                amount: Value {
                    lovelace: 0,
                    assets: Vec::new(),
                }
                .into(),
                stake_address,
                address_type,
                script: is_script,
            };

            let json = serde_json::to_string_pretty(&rest_response).map_err(|e| {
                RESTError::InternalServerError(format!("JSON serialization error: {e}"))
            })?;

            return Ok(RESTResponse::with_json(200, &json));
        }
        Err(e) => return Ok(RESTResponse::with_text(500, &format!("Query failed: {e}"))),
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
                "Unexpected  response querying UTxOs",
            )),
        },
    )
    .await?; // Let middleware handle the error

    let rest_response = AddressInfoREST {
        address: address_str.to_string(),
        amount: address_balance.into(),
        stake_address,
        address_type,
        script: is_script,
    };

    match serde_json::to_string_pretty(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving address info: {e}"),
        )),
    }
}

/// Handle `/addresses/{address}/extended` Blockfrost-compatible endpoint
pub async fn handle_address_extended_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

/// Handle `/addresses/{address}/totals` Blockfrost-compatible endpoint
pub async fn handle_address_totals_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

/// Handle `/addresses/{address}/utxos` Blockfrost-compatible endpoint
pub async fn handle_address_utxos_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

/// Handle `/addresses/{address}/utxos/{asset}` Blockfrost-compatible endpoint
pub async fn handle_address_asset_utxos_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

/// Handle `/addresses/{address}/transactions` Blockfrost-compatible endpoint
pub async fn handle_address_transactions_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}
