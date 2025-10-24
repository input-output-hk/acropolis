//! REST handlers for Acropolis Blockfrost /txs endpoints
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        transactions::{TransactionsStateQuery, TransactionsStateQueryResponse},
        utils::rest_query_state,
    },
    TxHash,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;
use hex::FromHex;
use std::sync::Arc;

use crate::handlers_config::HandlersConfig;

/// Handle `/txs/{hash}`
pub async fn handle_transactions_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    let tx_hash = match TxHash::from_hex(param) {
        Ok(hash) => hash,
        Err(_) => return Ok(RESTResponse::with_text(400, "Invalid transaction hash")),
    };

    let txs_info_msg = Arc::new(Message::StateQuery(StateQuery::Transactions(
        TransactionsStateQuery::GetTransactionInfo { tx_hash },
    )));
    rest_query_state(
        &context,
        &handlers_config.transactions_query_topic,
        txs_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Transactions(
                TransactionsStateQueryResponse::TransactionInfo(txs_info),
            )) => Some(Ok(Some(txs_info))),
            Message::StateQueryResponse(StateQueryResponse::Transactions(
                TransactionsStateQueryResponse::NotFound,
            )) => Some(Ok(None)),
            Message::StateQueryResponse(StateQueryResponse::Transactions(
                TransactionsStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}
