//! REST handlers for Acropolis Blockfrost /addresses endpoints
use std::sync::Arc;

use acropolis_common::app_error::RESTError;
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
use acropolis_common::serialization::serialize_to_json_response;
use crate::{handlers_config::HandlersConfig, types::AddressInfoREST};

/// Handle `/addresses/{address}` Blockfrost-compatible endpoint
pub async fn handle_address_single_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let address_str = params
        .first()
        .ok_or_else(|| RESTError::invalid_param("address", "parameter is missing"))?;

    let (address, stake_address) = parse_and_validate_address(address_str)?;

    let address_type = address.kind().to_string();
    let is_script = address.is_script();

    // Query for UTxOs at this address
    let address_query_msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressUTxOs { address },
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
                AddressStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(_),
            )) => Err(RESTError::storage_disabled("Address info")),
            _ => Err(RESTError::unexpected_response("retrieving address UTxOs")),
        },
    )
    .await?;

    // If no UTxOs found, return address with zero balance
    let utxo_identifiers = match utxo_identifiers {
        Some(ids) => ids,
        None => {
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
            return serialize_to_json_response(&rest_response);
        }
    };

    // Query for the sum of UTxOs to get total balance
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
                UTxOStateQueryResponse::NotFound,
            )) => Err(RESTError::not_found("UTxOs")),
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "UTxO query error: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response("querying UTxO sum")),
        },
    )
    .await?;

    let rest_response = AddressInfoREST {
        address: address_str.to_string(),
        amount: address_balance.into(),
        stake_address,
        address_type,
        script: is_script,
    };

    serialize_to_json_response(&rest_response)
}

/// Handle `/addresses/{address}/extended` Blockfrost-compatible endpoint
pub async fn handle_address_extended_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Address extended info"))
}

/// Handle `/addresses/{address}/totals` Blockfrost-compatible endpoint
pub async fn handle_address_totals_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Address totals"))
}

/// Handle `/addresses/{address}/utxos` Blockfrost-compatible endpoint
pub async fn handle_address_utxos_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Address UTxOs listing"))
}

/// Handle `/addresses/{address}/utxos/{asset}` Blockfrost-compatible endpoint
pub async fn handle_address_asset_utxos_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Address asset UTxOs"))
}

/// Handle `/addresses/{address}/transactions` Blockfrost-compatible endpoint
pub async fn handle_address_transactions_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Address transactions"))
}

/// Parse and validate an address string, extracting the address and optional stake address
fn parse_and_validate_address(address_str: &str) -> Result<(Address, Option<String>), RESTError> {
    let address = Address::from_string(address_str)
        .map_err(|e| RESTError::invalid_param("address", &format!("parse error: {}", e)))?;

    match address {
        Address::None | Address::Stake(_) => {
            Err(RESTError::invalid_param("address", "invalid address type"))
        }
        Address::Byron(byron) => Ok((Address::Byron(byron), None)),
        Address::Shelley(shelley) => {
            let stake_addr = shelley.stake_address_string().map_err(|e| {
                RESTError::invalid_param(
                    "address",
                    &format!("stake address extraction failed: {}", e),
                )
            })?;

            Ok((Address::Shelley(shelley), stake_addr))
        }
    }
}
