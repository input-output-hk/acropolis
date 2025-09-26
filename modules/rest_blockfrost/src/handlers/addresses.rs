use anyhow::Result;
use std::sync::Arc;

use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        addresses::{AddressStateQuery, AddressStateQueryResponse},
        utils::query_state,
    },
    Address,
};
use caryatid_sdk::Context;

use crate::handlers_config::HandlersConfig;

pub async fn handle_address_single_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let address = match Address::from_string(&params[0]) {
        Ok(Address::None) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid address '{}'", params[0]),
            ));
        }
        Ok(address) => address,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid address '{}': {e}", params[0]),
            ));
        }
    };

    let address_query_msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressUTxOs { address },
    )));

    let response = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        address_query_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressUTxOs(utxos),
            )) => {
                let rest_utxos: Vec<String> = utxos
                    .iter()
                    .map(|entry| {
                        format!(
                            "{}:{}:{}",
                            entry.block_number(),
                            entry.tx_index(),
                            entry.output_index()
                        )
                    })
                    .collect();

                match serde_json::to_string_pretty(&rest_utxos) {
                    Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                    Err(e) => Ok(RESTResponse::with_text(
                        500,
                        &format!("Failed to serialize UTxOs: {e}"),
                    )),
                }
            }
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::NotFound,
            )) => Ok(RESTResponse::with_text(404, "Address not found")),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(_),
            )) => Ok(RESTResponse::with_text(
                501,
                "Addresses info storage is disabled in config",
            )),
            _ => Ok(RESTResponse::with_text(
                500,
                "Unexpected response while retrieving address info",
            )),
        },
    )
    .await;

    match response {
        Ok(rest) => Ok(rest),
        Err(e) => Ok(RESTResponse::with_text(500, &format!("Query failed: {e}"))),
    }
}

pub async fn handle_address_extended_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_address_totals_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_address_utxos_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_address_asset_utxos_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_address_transactions_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}
