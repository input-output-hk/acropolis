//! REST handlers for Acropolis Blockfrost /txs endpoints
use acropolis_cardano::transaction::calculate_deposit;
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        parameters::{ParametersStateQuery, ParametersStateQueryResponse},
        transactions::{
            TransactionDelegationCertificate, TransactionInfo, TransactionStakeCertificate,
            TransactionsStateQuery, TransactionsStateQueryResponse,
        },
        utils::{query_state, rest_query_state_async},
    },
    Lovelace, TxHash,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;
use hex::FromHex;
use serde::{
    ser::{Error, SerializeStruct},
    Serialize, Serializer,
};
use std::sync::Arc;

use crate::handlers_config::HandlersConfig;

struct TxInfo(TransactionInfo, Lovelace, Lovelace);

impl Serialize for TxInfo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("TxInfo", 22)?;
        state.serialize_field("hash", &self.0.hash)?;
        state.serialize_field("block", &self.0.block_hash)?;
        state.serialize_field("height", &self.0.block_number)?;
        state.serialize_field("time", &self.0.block_time)?;
        state.serialize_field("slot", &self.0.slot)?;
        state.serialize_field("index", &self.0.index)?;
        state.serialize_field("output_amount", &self.0.output_amounts)?;
        state.serialize_field("fees", &self.1.to_string())?;
        state.serialize_field("deposit", &self.2.to_string())?;
        state.serialize_field("size", &self.0.size)?;
        state.serialize_field("invalid_before", &self.0.invalid_before)?;
        state.serialize_field("invalid_after", &self.0.invalid_after)?;
        state.serialize_field("utxo_count", &self.0.utxo_count)?;
        state.serialize_field("withdrawal_count", &self.0.withdrawal_count)?;
        state.serialize_field("mir_cert_count", &self.0.mir_cert_count)?;
        state.serialize_field("delegation_count", &self.0.delegation_count)?;
        state.serialize_field("stake_cert_count", &self.0.stake_cert_count)?;
        state.serialize_field("pool_update_count", &self.0.pool_update_count)?;
        state.serialize_field("pool_retire_count", &self.0.pool_retire_count)?;
        state.serialize_field("asset_mint_or_burn_count", &self.0.asset_mint_or_burn_count)?;
        state.serialize_field("redeemer_count", &self.0.redeemer_count)?;
        state.serialize_field("valid_contract", &self.0.valid_contract)?;
        state.end()
    }
}

/// Handle `/txs/{hash}`
pub async fn handle_transactions_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let (tx_hash, param, param2) = match params.as_slice() {
        [tx_hash] => (tx_hash, None, None),
        [tx_hash, param] => (tx_hash, Some(param.as_str()), None),
        [tx_hash, param, param2] => (tx_hash, Some(param.as_str()), Some(param2.as_str())),
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    let tx_hash = match TxHash::from_hex(tx_hash) {
        Ok(hash) => hash,
        Err(_) => return Ok(RESTResponse::with_text(400, "Invalid transaction hash")),
    };

    match param {
        None => handle_transaction_query(context, tx_hash, handlers_config).await,
        Some("utxo") => Ok(RESTResponse::with_text(501, "Not implemented")),
        Some("stakes") => handle_transaction_stakes_query(context, tx_hash, handlers_config).await,
        Some("delegations") => {
            handle_transaction_delegations_query(context, tx_hash, handlers_config).await
        }
        Some("withdrawals") => Ok(RESTResponse::with_text(501, "Not implemented")),
        Some("mirs") => Ok(RESTResponse::with_text(501, "Not implemented")),
        Some("pool_updates") => Ok(RESTResponse::with_text(501, "Not implemented")),
        Some("pool_retires") => Ok(RESTResponse::with_text(501, "Not implemented")),
        Some("metadata") => match param2 {
            None => Ok(RESTResponse::with_text(501, "Not implemented")),
            Some("cbor") => Ok(RESTResponse::with_text(501, "Not implemented")),
            _ => Ok(RESTResponse::with_text(400, "Invalid parameters")),
        },
        Some("redeemers") => Ok(RESTResponse::with_text(501, "Not implemented")),
        Some("required_signers") => Ok(RESTResponse::with_text(501, "Not implemented")),
        Some("cbor") => Ok(RESTResponse::with_text(501, "Not implemented")),
        _ => Ok(RESTResponse::with_text(400, "Invalid parameters")),
    }
}

/// Handle `/txs/{hash}`
async fn handle_transaction_query(
    context: Arc<Context<Message>>,
    tx_hash: TxHash,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let txs_info_msg = Arc::new(Message::StateQuery(StateQuery::Transactions(
        TransactionsStateQuery::GetTransactionInfo { tx_hash },
    )));
    rest_query_state_async(
        &context.clone(),
        &handlers_config.transactions_query_topic.clone(),
        txs_info_msg,
        async move |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Transactions(
                TransactionsStateQueryResponse::TransactionInfo(txs_info),
            )) => {
                let params_msg = Arc::new(Message::StateQuery(StateQuery::Parameters(
                    ParametersStateQuery::GetEpochParameters {
                        epoch_number: txs_info.epoch,
                    },
                )));
                let params = match query_state(
                    &context,
                    &handlers_config.parameters_query_topic,
                    params_msg,
                    |message| match message {
                        Message::StateQueryResponse(StateQueryResponse::Parameters(
                            ParametersStateQueryResponse::EpochParameters(params),
                        )) => Ok(params),
                        Message::StateQueryResponse(StateQueryResponse::Parameters(
                            ParametersStateQueryResponse::Error(e),
                        )) => Err(anyhow!(e)),
                        _ => Err(anyhow!("Unexpected response")),
                    },
                )
                .await
                {
                    Ok(params) => params,
                    Err(e) => return Some(Err(e)),
                };
                let fee = match txs_info.recorded_fee {
                    Some(fee) => fee,
                    None => 0, // TODO: calc from outputs and inputs
                };
                let deposit = match calculate_deposit(
                    txs_info.pool_update_count,
                    txs_info.stake_cert_count,
                    &params,
                ) {
                    Ok(deposit) => deposit,
                    Err(e) => return Some(Err(e)),
                };
                Some(Ok(Some(TxInfo(txs_info, fee, deposit))))
            }
            Message::StateQueryResponse(StateQueryResponse::Transactions(
                TransactionsStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

struct TxStake(TransactionStakeCertificate);

impl Serialize for TxStake {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let Ok(address) = self.0.address.to_string() else {
            return Err(S::Error::custom("Can't stringify address"));
        };
        let mut state = serializer.serialize_struct("TxStake", 3)?;
        state.serialize_field("index", &self.0.index)?;
        state.serialize_field("address", &address)?;
        state.serialize_field("registration", &self.0.registration)?;
        state.end()
    }
}

/// Handle `/txs/{hash}/stakes`
async fn handle_transaction_stakes_query(
    context: Arc<Context<Message>>,
    tx_hash: TxHash,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let txs_info_msg = Arc::new(Message::StateQuery(StateQuery::Transactions(
        TransactionsStateQuery::GetTransactionStakeCertificates { tx_hash },
    )));
    rest_query_state_async(
        &context.clone(),
        &handlers_config.transactions_query_topic.clone(),
        txs_info_msg,
        async move |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Transactions(
                TransactionsStateQueryResponse::TransactionStakeCertificates(stakes),
            )) => Some(Ok(Some(
                stakes.certificates.into_iter().map(TxStake).collect::<Vec<_>>(),
            ))),
            Message::StateQueryResponse(StateQueryResponse::Transactions(
                TransactionsStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}

struct TxDelegation(TransactionDelegationCertificate);

impl Serialize for TxDelegation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let Ok(address) = self.0.address.to_string() else {
            return Err(S::Error::custom("Can't stringify address"));
        };
        let mut state = serializer.serialize_struct("TxDelegation", 4)?;
        state.serialize_field("index", &self.0.index)?;
        state.serialize_field("address", &address)?;
        state.serialize_field("pool_id", &self.0.pool.to_string())?;
        state.serialize_field("active_epoch", &self.0.active_epoch)?;
        state.end()
    }
}

/// Handle `/txs/{hash}/delegations`
async fn handle_transaction_delegations_query(
    context: Arc<Context<Message>>,
    tx_hash: TxHash,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let txs_info_msg = Arc::new(Message::StateQuery(StateQuery::Transactions(
        TransactionsStateQuery::GetTransactionDelegationCertificates { tx_hash },
    )));
    rest_query_state_async(
        &context.clone(),
        &handlers_config.transactions_query_topic.clone(),
        txs_info_msg,
        async move |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Transactions(
                TransactionsStateQueryResponse::TransactionDelegationCertificates(delegations),
            )) => Some(Ok(Some(
                delegations.certificates.into_iter().map(TxDelegation).collect::<Vec<_>>(),
            ))),
            Message::StateQueryResponse(StateQueryResponse::Transactions(
                TransactionsStateQueryResponse::Error(e),
            )) => Some(Err(anyhow!(e))),
            _ => None,
        },
    )
    .await
}
