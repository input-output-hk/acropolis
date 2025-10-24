//! REST handlers for Acropolis Blockfrost /accounts endpoints
use std::sync::Arc;

use acropolis_common::messages::{Message, RESTResponse, StateQuery, StateQueryResponse};
use acropolis_common::queries::accounts::{AccountsStateQuery, AccountsStateQueryResponse};
use acropolis_common::queries::blocks::{
    BlocksStateQuery, BlocksStateQueryResponse, TransactionHashes,
};
use acropolis_common::queries::utils::query_state;
use acropolis_common::serialization::Bech32WithHrp;
use acropolis_common::{DRepChoice, StakeAddress};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;

use crate::handlers_config::HandlersConfig;
use crate::types::AccountWithdrawalREST;

#[derive(serde::Serialize)]
pub struct StakeAccountRest {
    pub utxo_value: u64,
    pub rewards: u64,
    pub delegated_spo: Option<String>,
    pub delegated_drep: Option<DRepChoiceRest>,
}

#[derive(serde::Serialize)]
pub struct DRepChoiceRest {
    pub drep_type: String,
    pub value: Option<String>,
}

/// Handle `/accounts/{stake_address}` Blockfrost-compatible endpoint
pub async fn handle_single_account_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(stake_key) = params.get(0) else {
        return Ok(RESTResponse::with_text(
            400,
            "Missing stake address parameter",
        ));
    };

    // Convert Bech32 stake address to StakeAddress
    let stake_address = match StakeAddress::from_string(&stake_key) {
        Ok(addr) => addr,
        _ => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Not a valid stake address: {stake_key}"),
            ));
        }
    };

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountInfo { stake_address },
    )));
    let account = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountInfo(account),
            )) => Ok(account),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => {
                return Err(anyhow::anyhow!("Account not found"));
            }
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving account info: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving account info"
                ))
            }
        },
    )
    .await?;

    let delegated_spo = match &account.delegated_spo {
        Some(spo) => match spo.to_bech32_with_hrp("pool") {
            Ok(val) => Some(val),
            Err(e) => {
                return Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while retrieving stake address: {e}"),
                ));
            }
        },
        None => None,
    };

    let delegated_drep = match &account.delegated_drep {
        Some(drep) => match map_drep_choice(drep) {
            Ok(val) => Some(val),
            Err(e) => {
                return Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while retrieving stake address: {e}"),
                ))
            }
        },
        None => None,
    };

    let rest_response = StakeAccountRest {
        utxo_value: account.utxo_value,
        rewards: account.rewards,
        delegated_spo,
        delegated_drep,
    };

    match serde_json::to_string(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving DRep delegation distribution: {e}"),
        )),
    }
}

pub async fn handle_account_withdrawals_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(stake_key) = params.get(0) else {
        return Ok(RESTResponse::with_text(
            400,
            "Missing stake address parameter",
        ));
    };

    // Convert Bech32 stake address to StakeAddress
    let stake_address = match StakeAddress::from_string(&stake_key) {
        Ok(addr) => addr,
        _ => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Not a valid stake address: {stake_key}"),
            ));
        }
    };

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountRegistrationHistory {
            account: stake_address,
        },
    )));

    // Get withdrawals from historical accounts state
    let withdrawals = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountWithdrawalHistory(registrations),
            )) => Ok(registrations),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => {
                return Err(anyhow::anyhow!("Account not found"));
            }
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving account info: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving account info"
                ))
            }
        },
    )
    .await?;

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = withdrawals.iter().map(|r| r.tx_identifier.clone()).collect();
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetTransactionHashes { tx_ids },
    )));
    let tx_hashes = query_state(
        &context,
        &handlers_config.blocks_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::TransactionHashes(TransactionHashes { tx_hashes }),
            )) => Ok(tx_hashes),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while resolving transaction hashes: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while resolving transaction hashes"
            )),
        },
    )
    .await?;

    let mut rest_response = Vec::new();

    for w in withdrawals {
        let Some(tx_hash) = tx_hashes.get(&w.tx_identifier) else {
            return Ok(RESTResponse::with_text(
                500,
                "Missing tx hash for withdrawal",
            ));
        };

        rest_response.push(AccountWithdrawalREST {
            tx_hash: hex::encode(tx_hash),
            amount: w.amount.to_string(),
        });
    }

    match serde_json::to_string(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving DRep delegation distribution: {e}"),
        )),
    }
}

fn map_drep_choice(drep: &DRepChoice) -> Result<DRepChoiceRest> {
    match drep {
        DRepChoice::Key(hash) => {
            let val = hash
                .to_bech32_with_hrp("drep")
                .map_err(|e| anyhow!("Bech32 encoding failed for DRep Key: {e}"))?;
            Ok(DRepChoiceRest {
                drep_type: "Key".to_string(),
                value: Some(val),
            })
        }
        DRepChoice::Script(hash) => {
            let val = hash
                .to_bech32_with_hrp("drep_script")
                .map_err(|e| anyhow!("Bech32 encoding failed for DRep Script: {e}"))?;
            Ok(DRepChoiceRest {
                drep_type: "Script".to_string(),
                value: Some(val),
            })
        }
        DRepChoice::Abstain => Ok(DRepChoiceRest {
            drep_type: "Abstain".to_string(),
            value: None,
        }),
        DRepChoice::NoConfidence => Ok(DRepChoiceRest {
            drep_type: "NoConfidence".to_string(),
            value: None,
        }),
    }
}
