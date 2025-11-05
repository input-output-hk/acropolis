//! REST handlers for Acropolis Blockfrost /accounts endpoints
use std::sync::Arc;

use acropolis_common::app_error::RESTError;
use acropolis_common::messages::{Message, RESTResponse, StateQuery, StateQueryResponse};
use acropolis_common::queries::accounts::{AccountsStateQuery, AccountsStateQueryResponse};
use acropolis_common::queries::blocks::{
    BlocksStateQuery, BlocksStateQueryResponse, TransactionHashes,
};
use acropolis_common::queries::utils::{query_state, serialize_to_json_response};
use acropolis_common::serialization::{Bech32Conversion, Bech32WithHrp};
use acropolis_common::{DRepChoice, StakeAddress, TxHash};
use caryatid_sdk::Context;

use crate::handlers_config::HandlersConfig;
use crate::types::{
    AccountRewardREST, AccountWithdrawalREST, DelegationUpdateREST, RegistrationUpdateREST,
};

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
            )) => Ok(Some(account)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving account info: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response("retrieving account info")),
        },
    )
    .await?;

    let account = account.ok_or_else(|| RESTError::not_found("Account"))?;

    let delegated_spo = account
        .delegated_spo
        .as_ref()
        .map(|spo| spo.to_bech32())
        .transpose()
        .map_err(|e| RESTError::encoding_failed(&format!("SPO: {}", e)))?;

    let delegated_drep = account.delegated_drep.as_ref().map(map_drep_choice).transpose()?;

    let rest_response = StakeAccountRest {
        utxo_value: account.utxo_value,
        rewards: account.rewards,
        delegated_spo,
        delegated_drep,
    };

    serialize_to_json_response(&rest_response)
}

/// Handle `/accounts/{stake_address}/registrations` Blockfrost-compatible endpoint
pub async fn handle_account_registrations_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountRegistrationHistory { account },
    )));

    let registrations = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountRegistrationHistory(registrations),
            )) => Ok(Some(registrations)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving account registrations: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response(
                "retrieving account registrations",
            )),
        },
    )
    .await?;

    let registrations = registrations.ok_or_else(|| RESTError::not_found("Account"))?;

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = registrations.iter().map(|r| r.tx_identifier).collect();
    let tx_hashes = fetch_transaction_hashes(&context, &handlers_config, tx_ids).await?;

    let rest_response: Vec<RegistrationUpdateREST> = registrations
        .iter()
        .map(|r| {
            let tx_hash = tx_hashes
                .get(&r.tx_identifier)
                .ok_or_else(|| RESTError::not_found("Transaction hash for registration"))?;

            Ok(RegistrationUpdateREST {
                tx_hash: hex::encode(tx_hash),
                action: r.status.to_string(),
            })
        })
        .collect::<Result<Vec<_>, RESTError>>()?;

    serialize_to_json_response(&rest_response)
}

/// Handle `/accounts/{stake_address}/delegations` Blockfrost-compatible endpoint
pub async fn handle_account_delegations_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountDelegationHistory { account },
    )));

    let delegations = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountDelegationHistory(delegations),
            )) => Ok(Some(delegations)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving account delegations: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response(
                "retrieving account delegations",
            )),
        },
    )
    .await?;

    let delegations = delegations.ok_or_else(|| RESTError::not_found("Account"))?;

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = delegations.iter().map(|r| r.tx_identifier).collect();
    let tx_hashes = fetch_transaction_hashes(&context, &handlers_config, tx_ids).await?;

    let rest_response: Vec<DelegationUpdateREST> = delegations
        .iter()
        .map(|r| {
            let tx_hash = tx_hashes
                .get(&r.tx_identifier)
                .ok_or_else(|| RESTError::not_found("Transaction hash for delegation"))?;

            let pool_id = r
                .pool
                .to_bech32()
                .map_err(|e| RESTError::encoding_failed(&format!("pool ID: {}", e)))?;

            Ok(DelegationUpdateREST {
                active_epoch: r.active_epoch,
                tx_hash: hex::encode(tx_hash),
                amount: r.amount.to_string(),
                pool_id,
            })
        })
        .collect::<Result<Vec<_>, RESTError>>()?;

    serialize_to_json_response(&rest_response)
}

/// Handle `/accounts/{stake_address}/mirs` Blockfrost-compatible endpoint
pub async fn handle_account_mirs_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountMIRHistory { account },
    )));

    let mirs = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountMIRHistory(mirs),
            )) => Ok(Some(mirs)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving account MIRs: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response("retrieving account MIRs")),
        },
    )
    .await?;

    let mirs = mirs.ok_or_else(|| RESTError::not_found("Account"))?;

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = mirs.iter().map(|r| r.tx_identifier).collect();
    let tx_hashes = fetch_transaction_hashes(&context, &handlers_config, tx_ids).await?;

    let rest_response: Vec<AccountWithdrawalREST> = mirs
        .iter()
        .map(|r| {
            let tx_hash = tx_hashes
                .get(&r.tx_identifier)
                .ok_or_else(|| RESTError::not_found("Transaction hash for MIR record"))?;

            Ok(AccountWithdrawalREST {
                tx_hash: hex::encode(tx_hash),
                amount: r.amount.to_string(),
            })
        })
        .collect::<Result<Vec<_>, RESTError>>()?;

    serialize_to_json_response(&rest_response)
}

/// Handle `/accounts/{stake_address}/withdrawals` Blockfrost-compatible endpoint
pub async fn handle_account_withdrawals_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountWithdrawalHistory { account },
    )));

    let withdrawals = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountWithdrawalHistory(withdrawals),
            )) => Ok(Some(withdrawals)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving account withdrawals: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response(
                "retrieving account withdrawals",
            )),
        },
    )
    .await?;

    let withdrawals = withdrawals.ok_or_else(|| RESTError::not_found("Account"))?;

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = withdrawals.iter().map(|r| r.tx_identifier).collect();
    let tx_hashes = fetch_transaction_hashes(&context, &handlers_config, tx_ids).await?;

    let rest_response: Vec<AccountWithdrawalREST> = withdrawals
        .iter()
        .map(|w| {
            let tx_hash = tx_hashes
                .get(&w.tx_identifier)
                .ok_or_else(|| RESTError::not_found("Transaction hash for withdrawal"))?;

            Ok(AccountWithdrawalREST {
                tx_hash: hex::encode(tx_hash),
                amount: w.amount.to_string(),
            })
        })
        .collect::<Result<Vec<_>, RESTError>>()?;

    serialize_to_json_response(&rest_response)
}

/// Handle `/accounts/{stake_address}/rewards` Blockfrost-compatible endpoint
pub async fn handle_account_rewards_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountRewardHistory { account },
    )));

    let rewards = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountRewardHistory(rewards),
            )) => Ok(Some(rewards)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error retrieving account rewards: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response("retrieving account rewards")),
        },
    )
    .await?;

    let rewards = rewards.ok_or_else(|| RESTError::not_found("Account"))?;

    let rest_response: Vec<AccountRewardREST> =
        rewards.iter().map(|r| r.try_into()).collect::<Result<Vec<_>, _>>().map_err(|e| {
            RESTError::InternalServerError(format!("Failed to convert reward entry: {}", e))
        })?;

    serialize_to_json_response(&rest_response)
}

/// Parse and validate a stake address from request parameters
fn parse_stake_address(params: &[String]) -> Result<StakeAddress, RESTError> {
    let stake_key = params
        .first()
        .ok_or_else(|| RESTError::invalid_param("stake_address", "parameter is missing"))?;

    StakeAddress::from_string(stake_key)
        .map_err(|_| RESTError::invalid_param("stake_address", "not a valid stake address"))
}

/// Fetch transaction hashes for a list of transaction identifiers
async fn fetch_transaction_hashes(
    context: &Arc<Context<Message>>,
    handlers_config: &HandlersConfig,
    tx_ids: Vec<impl Into<acropolis_common::TxIdentifier> + Clone>,
) -> Result<
    std::collections::HashMap<acropolis_common::TxIdentifier, TxHash>,
    RESTError,
> {
    let tx_ids = tx_ids.into_iter().map(|id| id.into()).collect();
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetTransactionHashes { tx_ids },
    )));

    query_state(
        context,
        &handlers_config.blocks_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::TransactionHashes(TransactionHashes { tx_hashes }),
            )) => Ok(tx_hashes),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(RESTError::InternalServerError(format!(
                "Error resolving transaction hashes: {}",
                e
            ))),
            _ => Err(RESTError::unexpected_response(
                "resolving transaction hashes",
            )),
        },
    )
    .await
}

/// Map a DRepChoice to its REST representation
fn map_drep_choice(drep: &DRepChoice) -> Result<DRepChoiceRest, RESTError> {
    match drep {
        DRepChoice::Key(hash) => {
            let val = hash
                .to_vec()
                .to_bech32_with_hrp("drep")
                .map_err(|e| RESTError::encoding_failed(&format!("DRep Key: {}", e)))?;
            Ok(DRepChoiceRest {
                drep_type: "Key".to_string(),
                value: Some(val),
            })
        }
        DRepChoice::Script(hash) => {
            let val = hash
                .to_vec()
                .to_bech32_with_hrp("drep_script")
                .map_err(|e| RESTError::encoding_failed(&format!("DRep Script: {}", e)))?;
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
