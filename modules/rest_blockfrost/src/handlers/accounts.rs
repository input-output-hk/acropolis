//! REST handlers for Acropolis Blockfrost /accounts endpoints
use std::sync::Arc;

use acropolis_common::messages::{Message, RESTResponse, StateQuery, StateQueryResponse};
use acropolis_common::queries::accounts::{AccountsStateQuery, AccountsStateQueryResponse};
use acropolis_common::queries::addresses::{AddressStateQuery, AddressStateQueryResponse};
use acropolis_common::queries::blocks::{
    BlocksStateQuery, BlocksStateQueryResponse, TransactionHashes,
};
use acropolis_common::queries::utils::query_state;
use acropolis_common::queries::utxos::UTxOStateQuery;
use acropolis_common::serialization::{Bech32Conversion, Bech32WithHrp};
use acropolis_common::{DRepChoice, StakeAddress};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;

use crate::handlers_config::HandlersConfig;
use crate::types::{
    AccountAddressREST, AccountRewardREST, AccountWithdrawalREST, DelegationUpdateREST,
    RegistrationUpdateREST,
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
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };
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
            )) => Ok(Some(account)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account info: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account info"
            )),
        },
    )
    .await?;

    let Some(account) = account else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    let delegated_spo = match &account.delegated_spo {
        Some(spo) => match spo.to_bech32() {
            Ok(val) => Some(val),
            Err(e) => {
                return Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while mapping SPO: {e}"),
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
                    &format!("Internal server error while mapping dRep: {e}"),
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

    match serde_json::to_string_pretty(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving account info: {e}"),
        )),
    }
}

/// Handle `/accounts/{stake_address}/registrations` Blockfrost-compatible endpoint
pub async fn handle_account_registrations_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

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
            )) => Ok(Some(registrations)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account registrations: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account registrations"
            )),
        },
    )
    .await?;

    let Some(registrations) = registrations else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = registrations.iter().map(|r| r.tx_identifier).collect();
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

    for r in registrations {
        let Some(tx_hash) = tx_hashes.get(&r.tx_identifier) else {
            return Ok(RESTResponse::with_text(
                500,
                "Missing tx hash for registration",
            ));
        };

        rest_response.push(RegistrationUpdateREST {
            tx_hash: hex::encode(tx_hash),
            action: r.status.to_string(),
        });
    }

    match serde_json::to_string_pretty(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while serializing registration history: {e}"),
        )),
    }
}

/// Handle `/accounts/{stake_address}/delegations` Blockfrost-compatible endpoint
pub async fn handle_account_delegations_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

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
            )) => Ok(Some(delegations)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account delegations: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account delegations"
            )),
        },
    )
    .await?;

    let Some(delegations) = delegations else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = delegations.iter().map(|r| r.tx_identifier).collect();
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

    for r in delegations {
        let Some(tx_hash) = tx_hashes.get(&r.tx_identifier) else {
            return Ok(RESTResponse::with_text(
                500,
                "Missing tx hash for delegation",
            ));
        };

        let pool_id = match r.pool.to_bech32() {
            Ok(p) => p,
            Err(e) => {
                return Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to encode pool ID: {e}"),
                ));
            }
        };

        rest_response.push(DelegationUpdateREST {
            active_epoch: r.active_epoch,
            tx_hash: hex::encode(tx_hash),
            amount: r.amount.to_string(),
            pool_id,
        });
    }

    match serde_json::to_string_pretty(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while serializing delegation history: {e}"),
        )),
    }
}

/// Handle `/accounts/{stake_address}/mirs` Blockfrost-compatible endpoint
pub async fn handle_account_mirs_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

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
            )) => Ok(Some(mirs)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account mirs: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account mirs"
            )),
        },
    )
    .await?;

    let Some(mirs) = mirs else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = mirs.iter().map(|r| r.tx_identifier).collect();
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

    for r in mirs {
        let Some(tx_hash) = tx_hashes.get(&r.tx_identifier) else {
            return Ok(RESTResponse::with_text(
                500,
                "Missing tx hash for MIR record",
            ));
        };

        rest_response.push(AccountWithdrawalREST {
            tx_hash: hex::encode(tx_hash),
            amount: r.amount.to_string(),
        });
    }

    match serde_json::to_string_pretty(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while serializing MIR history: {e}"),
        )),
    }
}

pub async fn handle_account_withdrawals_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountRegistrationHistory { account },
    )));

    // Get withdrawals from historical accounts state
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
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account withdrawals: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account withdrawals"
            )),
        },
    )
    .await?;

    let Some(withdrawals) = withdrawals else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    // Get TxHashes from TxIdentifiers
    let tx_ids: Vec<_> = withdrawals.iter().map(|r| r.tx_identifier).collect();
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

    match serde_json::to_string_pretty(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while serializing withdrawal history: {e}"),
        )),
    }
}

pub async fn handle_account_rewards_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

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
            )) => Ok(Some(rewards)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account rewards: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account rewards"
            )),
        },
    )
    .await?;

    let Some(rewards) = rewards else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    let rest_response =
        match rewards.iter().map(|r| r.try_into()).collect::<Result<Vec<AccountRewardREST>, _>>() {
            Ok(v) => v,
            Err(e) => {
                return Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to convert reward entry: {e}"),
                ))
            }
        };

    match serde_json::to_string_pretty(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while serializing reward history: {e}"),
        )),
    }
}

pub async fn handle_account_addresses_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

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
            )) => Ok(Some(addresses)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account addresses: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account addresses"
            )),
        },
    )
    .await?;

    let Some(addresses) = addresses else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    let rest_response = match addresses
        .iter()
        .map(|r| {
            Ok::<_, anyhow::Error>(AccountAddressREST {
                address: r.to_string().map_err(|e| anyhow!("invalid address: {e}"))?,
            })
        })
        .collect::<Result<Vec<AccountAddressREST>, _>>()
    {
        Ok(v) => v,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Failed to convert address entry: {e}"),
            ));
        }
    };

    match serde_json::to_string_pretty(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while serializing addresses: {e}"),
        )),
    }
}

/// Handle `/accounts/{stake_address}/addresses/assets` Blockfrost-compatible endpoint
pub async fn handle_account_assets_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

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
            )) => Ok(Some(addresses)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account addresses: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account addresses"
            )),
        },
    )
    .await?;

    let Some(addresses) = addresses else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressesAssets { addresses },
    )));

    // Get assets from address state
    let _assets = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressesAssets(assets),
            )) => Ok(Some(assets)),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account assets: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account assets"
            )),
        },
    )
    .await?;
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

/// Handle `/accounts/{stake_address}/addresses/total` Blockfrost-compatible endpoint
pub async fn handle_account_totals_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

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
            )) => Ok(Some(addresses)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account addresses: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account addresses"
            )),
        },
    )
    .await?;

    let Some(addresses) = addresses else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressesTotals { addresses },
    )));

    // Get totals from address state
    let _totals = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressesTotals(totals),
            )) => Ok(Some(totals)),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account totals: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account totals"
            )),
        },
    )
    .await?;
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

/// Handle `/accounts/{stake_address}/utxos` Blockfrost-compatible endpoint
pub async fn handle_account_utxos_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let account = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

    // Get addresses from historical accounts state
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountAssociatedAddresses { account },
    )));
    let addresses = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountAssociatedAddresses(addresses),
            )) => Ok(Some(addresses)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account addresses: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account addresses"
            )),
        },
    )
    .await?;

    let Some(addresses) = addresses else {
        return Ok(RESTResponse::with_text(404, "Account not found"));
    };

    // Get utxos from address state
    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressesUTxOs { addresses },
    )));
    let utxo_identifiers = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressesUTxOs(utxos),
            )) => Ok(utxos),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account UTxOs: No UTxOs found"
            )),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account UTxOs: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account UTxOs"
            )),
        },
    )
    .await?;

    // Get UTxO balances from utxo state
    let msg = Arc::new(Message::StateQuery(StateQuery::UTxOs(
        UTxOStateQuery::GetUTxOsMap { utxo_identifiers },
    )));
    let balances = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressesUTxOs(utxos),
            )) => Ok(Some(utxos)),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::NotFound,
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving account UTxOs: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving account UTxOs"
            )),
        },
    )
    .await?;
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

fn parse_stake_address(params: &[String]) -> Result<StakeAddress, RESTResponse> {
    let Some(stake_key) = params.first() else {
        return Err(RESTResponse::with_text(
            400,
            "Missing stake address parameter",
        ));
    };

    StakeAddress::from_string(stake_key).map_err(|_| {
        RESTResponse::with_text(400, &format!("Not a valid stake address: {stake_key}"))
    })
}

fn map_drep_choice(drep: &DRepChoice) -> Result<DRepChoiceRest> {
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
