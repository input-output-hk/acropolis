use std::sync::Arc;

use crate::types::{AddressTotalsREST, TransactionInfoREST, UTxOREST};
use crate::utils::split_policy_and_asset;
use crate::{handlers_config::HandlersConfig, types::AddressInfoREST};
use acropolis_common::queries::blocks::{BlocksStateQuery, BlocksStateQueryResponse};
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
    let address = parse_address(&params)?;
    let stake_address = match address {
        Address::Shelley(ref addr) => addr.stake_address_string()?,
        _ => None,
    };

    let address_type = address.kind().to_string();
    let is_script = address.is_script();

    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressUTxOs {
            address: address.clone(),
        },
    )));

    let utxo_identifiers = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        msg,
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

    let msg = Arc::new(Message::StateQuery(StateQuery::UTxOs(
        UTxOStateQuery::GetUTxOsSum { utxo_identifiers },
    )));

    let address_balance = query_state(
        &context,
        &handlers_config.utxos_query_topic,
        msg,
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
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    Err(RESTError::not_implemented("Address extended endpoint"))
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
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let address = parse_address(&params)?;
    let address_str = address.to_string()?;

    // Get utxos from address state
    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressUTxOs { address },
    )));
    let utxo_identifiers = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressUTxOs(utxos),
            )) => Ok(utxos),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving address UTxOs",
            )),
        },
    )
    .await?;

    // Get TxHashes and BlockHashes from UTxOIdentifiers
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetUTxOHashes {
            utxo_ids: utxo_identifiers.clone(),
        },
    )));
    let hashes = query_state(
        &context,
        &handlers_config.blocks_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::UTxOHashes(hashes),
            )) => Ok(hashes),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving UTxO hashes",
            )),
        },
    )
    .await?;

    // Get UTxO balances from utxo state
    let msg = Arc::new(Message::StateQuery(StateQuery::UTxOs(
        UTxOStateQuery::GetUTxOs {
            utxo_identifiers: utxo_identifiers.clone(),
        },
    )));
    let entries = query_state(
        &context,
        &handlers_config.utxos_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::UTxOs(utxos),
            )) => Ok(utxos),
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving UTxO entries",
            )),
        },
    )
    .await?;

    let mut rest_response = Vec::with_capacity(entries.len());
    for (i, entry) in entries.into_iter().enumerate() {
        rest_response.push(UTxOREST::new(
            address_str.clone(),
            &utxo_identifiers[i],
            &entry,
            hashes.tx_hashes[i].as_ref(),
            hashes.block_hashes[i].as_ref(),
        ))
    }

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/addresses/{address}/utxos/{asset}` Blockfrost-compatible endpoint
pub async fn handle_address_asset_utxos_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let address = parse_address(&params)?;
    let address_str = address.to_string()?;
    let (target_policy, target_name) = split_policy_and_asset(&params[1])?;

    // Get utxos from address state
    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressUTxOs { address },
    )));
    let utxo_identifiers = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressUTxOs(utxos),
            )) => Ok(utxos),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving address UTxOs",
            )),
        },
    )
    .await?;

    // Get UTxO balances from utxo state
    let msg = Arc::new(Message::StateQuery(StateQuery::UTxOs(
        UTxOStateQuery::GetUTxOs {
            utxo_identifiers: utxo_identifiers.clone(),
        },
    )));
    let entries = query_state(
        &context,
        &handlers_config.utxos_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::UTxOs(utxos),
            )) => Ok(utxos),
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving UTxO entries",
            )),
        },
    )
    .await?;

    // Filter for UTxOs which contain the asset
    let mut filtered_identifiers = Vec::new();
    let mut filtered_entries = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        let matches = entry.value.assets.iter().any(|(policy, assets)| {
            policy == &target_policy && assets.iter().any(|asset| asset.name == target_name)
        });

        if matches {
            filtered_identifiers.push(utxo_identifiers[i]);
            filtered_entries.push(entry);
        }
    }

    if filtered_identifiers.is_empty() {
        return Ok(RESTResponse::with_json(200, "[]"));
    }

    // Get TxHashes and BlockHashes from subset of UTxOIdentifiers with specific asset balances
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetUTxOHashes {
            utxo_ids: filtered_identifiers.clone(),
        },
    )));
    let hashes = query_state(
        &context,
        &handlers_config.blocks_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::UTxOHashes(hashes),
            )) => Ok(hashes),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving UTxO hashes",
            )),
        },
    )
    .await?;

    let mut rest_response = Vec::with_capacity(filtered_entries.len());
    for (i, entry) in filtered_entries.into_iter().enumerate() {
        rest_response.push(UTxOREST::new(
            address_str.clone(),
            &filtered_identifiers[i],
            entry,
            hashes.tx_hashes[i].as_ref(),
            hashes.block_hashes[i].as_ref(),
        ))
    }

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/addresses/{address}/transactions` Blockfrost-compatible endpoint
pub async fn handle_address_transactions_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let address = parse_address(&params)?;

    // Get tx identifiers from address state
    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressTransactions { address },
    )));
    let tx_identifiers = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressTransactions(txs),
            )) => Ok(txs),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving address transactions",
            )),
        },
    )
    .await?;

    // Get tx hashes and timestamps from chain store
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetTransactionHashesAndTimestamps {
            tx_ids: tx_identifiers.clone(),
        },
    )));
    let tx_info = query_state(
        &context,
        &handlers_config.blocks_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::TransactionHashesAndTimestamps(info),
            )) => Ok(info),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving transaction hashes and timestamps",
            )),
        },
    )
    .await?;

    let mut rest_response = Vec::with_capacity(tx_identifiers.len());
    for (i, tx_id) in tx_identifiers.iter().enumerate() {
        rest_response.push(TransactionInfoREST {
            tx_hash: hex::encode(tx_info.tx_hashes[i]),
            tx_index: tx_id.tx_index(),
            block_height: tx_id.block_number(),
            block_time: tx_info.timestamps[i],
        });
    }

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

fn parse_address(params: &[String]) -> Result<Address, RESTError> {
    let Some(address_str) = params.first() else {
        return Err(RESTError::param_missing("address"));
    };

    Ok(Address::from_string(address_str)?)
}
