//! REST handlers for Acropolis Blockfrost /accounts endpoints
use std::sync::Arc;

use crate::handlers_config::HandlersConfig;
use crate::types::{
    AccountAddressREST, AccountRewardREST, AccountWithdrawalREST, DelegationUpdateREST,
    RegistrationUpdateREST,
};
use acropolis_common::messages::{Message, RESTResponse, StateQuery, StateQueryResponse};
use acropolis_common::queries::accounts::{AccountsStateQuery, AccountsStateQueryResponse};
use acropolis_common::queries::blocks::{
    BlocksStateQuery, BlocksStateQueryResponse, TransactionHashes,
};
use acropolis_common::queries::errors::QueryError;
use acropolis_common::queries::utils::query_state;
use acropolis_common::rest_error::RESTError;
use acropolis_common::serialization::{Bech32Conversion, Bech32WithHrp};
use acropolis_common::{DRepChoice, StakeAddress, TxHash};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;

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
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountInfo { account },
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
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving account info",
            )),
        },
    )
    .await?;

    let delegated_spo = match &account.delegated_spo {
        Some(spo) => {
            Some(spo.to_bech32().map_err(|e| RESTError::encoding_failed(&format!("SPO: {e}")))?)
        }
        None => None,
    };

    let delegated_drep = match &account.delegated_drep {
        Some(drep) => Some(
            map_drep_choice(drep).map_err(|e| RESTError::encoding_failed(&format!("dRep: {e}")))?,
        ),
        None => None,
    };

    let rest_response = StakeAccountRest {
        utxo_value: account.utxo_value,
        rewards: account.rewards,
        delegated_spo,
        delegated_drep,
    };

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/accounts/{stake_address}/registrations` Blockfrost-compatible endpoint
pub async fn handle_account_registrations_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountRegistrationHistory { account },
    )));

    // Get registrations from historical accounts state
    let registrations = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountRegistrationHistory(registrations),
            )) => Ok(registrations),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed(
                "Unexpected message type while retrieving account registrations",
            )),
        },
    )
    .await?;

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = registrations.iter().map(|r| r.tx_identifier).collect();
    let tx_hashes = get_transaction_hashes(&context, &handlers_config, tx_ids).await?;

    let mut rest_response = Vec::new();

    for r in registrations {
        let tx_hash = tx_hashes.get(&r.tx_identifier).ok_or_else(|| {
            RESTError::InternalServerError("Missing tx hash for registration".to_string())
        })?;

        rest_response.push(RegistrationUpdateREST {
            tx_hash: hex::encode(tx_hash),
            action: r.status.to_string(),
        });
    }

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/accounts/{stake_address}/delegations` Blockfrost-compatible endpoint
pub async fn handle_account_delegations_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountDelegationHistory { account },
    )));

    // Get delegations from historical accounts state
    let delegations = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountDelegationHistory(delegations),
            )) => Ok(delegations),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed("Unexpected response type")),
        },
    )
    .await?;

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = delegations.iter().map(|r| r.tx_identifier).collect();
    let tx_hashes = get_transaction_hashes(&context, &handlers_config, tx_ids).await?;

    let mut rest_response = Vec::new();

    for r in delegations {
        let tx_hash = tx_hashes.get(&r.tx_identifier).ok_or_else(|| {
            RESTError::InternalServerError("Missing tx hash for delegation".to_string())
        })?;

        let pool_id =
            r.pool.to_bech32().map_err(|e| RESTError::encoding_failed(&format!("pool ID: {e}")))?;

        rest_response.push(DelegationUpdateREST {
            active_epoch: r.active_epoch,
            tx_hash: hex::encode(tx_hash),
            amount: r.amount.to_string(),
            pool_id,
        });
    }

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/accounts/{stake_address}/mirs` Blockfrost-compatible endpoint
pub async fn handle_account_mirs_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountMIRHistory { account },
    )));

    // Get MIRs from historical accounts state
    let mirs = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountMIRHistory(mirs),
            )) => Ok(mirs),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed("Unexpected response type")),
        },
    )
    .await?;

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = mirs.iter().map(|r| r.tx_identifier).collect();
    let tx_hashes = get_transaction_hashes(&context, &handlers_config, tx_ids).await?;

    let mut rest_response = Vec::new();

    for r in mirs {
        let tx_hash = tx_hashes.get(&r.tx_identifier).ok_or_else(|| {
            RESTError::InternalServerError("Missing tx hash for MIR record".to_string())
        })?;

        rest_response.push(AccountWithdrawalREST {
            tx_hash: hex::encode(tx_hash),
            amount: r.amount.to_string(),
        });
    }

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_account_withdrawals_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountWithdrawalHistory { account },
    )));

    // Get withdrawals from historical accounts state
    let withdrawals = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountWithdrawalHistory(withdrawals),
            )) => Ok(withdrawals),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed("Unexpected response type")),
        },
    )
    .await?;

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = withdrawals.iter().map(|r| r.tx_identifier).collect();
    let tx_hashes = get_transaction_hashes(&context, &handlers_config, tx_ids).await?;

    let mut rest_response = Vec::new();

    for w in withdrawals {
        let tx_hash = tx_hashes.get(&w.tx_identifier).ok_or_else(|| {
            RESTError::InternalServerError("Missing tx hash for withdrawal".to_string())
        })?;

        rest_response.push(AccountWithdrawalREST {
            tx_hash: hex::encode(tx_hash),
            amount: w.amount.to_string(),
        });
    }

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_account_rewards_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountRewardHistory { account },
    )));

    // Get rewards from historical accounts state
    let rewards = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountRewardHistory(rewards),
            )) => Ok(rewards),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed("Unexpected response type")),
        },
    )
    .await?;

    let rest_response = rewards
        .iter()
        .map(|r| r.try_into())
        .collect::<Result<Vec<AccountRewardREST>, _>>()
        .map_err(|e: anyhow::Error| {
            RESTError::InternalServerError(format!("Failed to convert reward entry: {e}"))
        })?;

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_account_addresses_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountAssociatedAddresses { account },
    )));

    // Get addresses from historical accounts state
    let addresses = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountAssociatedAddresses(addresses),
            )) => Ok(addresses),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed("Unexpected response type")),
        },
    )
    .await?;

    let rest_response = addresses
        .iter()
        .map(|r| {
            Ok::<_, RESTError>(AccountAddressREST {
                address: r
                    .to_string()
                    .map_err(|e| RESTError::InternalServerError(format!("Invalid address: {e}")))?,
            })
        })
        .collect::<Result<Vec<AccountAddressREST>, RESTError>>()?;

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

fn parse_stake_address(params: &[String]) -> Result<StakeAddress, RESTError> {
    let stake_key = params.first().ok_or_else(|| RESTError::param_missing("stake address"))?;

    StakeAddress::from_string(stake_key)
        .map_err(|_| RESTError::invalid_param("stake address", stake_key))
}

fn map_drep_choice(drep: &DRepChoice) -> Result<DRepChoiceRest, anyhow::Error> {
    match drep {
        DRepChoice::Key(hash) => {
            let val = hash
                .to_vec()
                .to_bech32_with_hrp("drep")
                .map_err(|e| anyhow!("Bech32 encoding failed for DRep Key: {e}"))?;
            Ok(DRepChoiceRest {
                drep_type: "Key".to_string(),
                value: Some(val),
            })
        }
        DRepChoice::Script(hash) => {
            let val = hash
                .to_vec()
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

/// Helper to fetch transaction hashes (used by multiple handlers)
async fn get_transaction_hashes(
    context: &Arc<Context<Message>>,
    handlers_config: &Arc<HandlersConfig>,
    tx_ids: Vec<acropolis_common::TxIdentifier>,
) -> Result<std::collections::HashMap<acropolis_common::TxIdentifier, TxHash>, RESTError> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetTransactionHashes { tx_ids },
    )));

    let result = query_state(
        context,
        &handlers_config.blocks_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::TransactionHashes(TransactionHashes { tx_hashes }),
            )) => Ok(tx_hashes),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::query_failed("Unexpected response type")),
        },
    )
    .await?;

    Ok(result)
}
