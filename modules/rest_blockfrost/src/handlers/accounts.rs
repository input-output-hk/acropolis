//! REST handlers for Acropolis Blockfrost /accounts endpoints
use std::sync::Arc;

use crate::handlers_config::HandlersConfig;
use crate::types::{
    AccountAddressREST, AccountRewardREST, AccountTotalsREST, AccountUTxOREST,
    AccountWithdrawalREST, AmountList, DelegationUpdateREST, RegistrationUpdateREST,
};
use acropolis_common::messages::{Message, RESTResponse, StateQuery, StateQueryResponse};
use acropolis_common::queries::accounts::{AccountsStateQuery, AccountsStateQueryResponse};
use acropolis_common::queries::addresses::{AddressStateQuery, AddressStateQueryResponse};
use acropolis_common::queries::blocks::{
    BlocksStateQuery, BlocksStateQueryResponse, TransactionHashes,
};
use acropolis_common::queries::errors::QueryError;
use acropolis_common::queries::utils::query_state;
use acropolis_common::queries::utxos::{UTxOStateQuery, UTxOStateQueryResponse};
use acropolis_common::rest_error::RESTError;
use acropolis_common::serialization::{Bech32Conversion, Bech32WithHrp};
use acropolis_common::{DRepChoice, Datum, ReferenceScript, StakeAddress};
use blake2::{Blake2b512, Digest};
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
            )) => Ok(Some(account)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account info",
            )),
        },
    )
    .await?;

    let Some(account) = account else {
        return Err(RESTError::not_found("Account not found"));
    };

    let delegated_spo = account
        .delegated_spo
        .as_ref()
        .map(|spo| spo.to_bech32())
        .transpose()
        .map_err(|e| RESTError::encoding_failed(&format!("SPO: {e}")))?;

    let delegated_drep = account
        .delegated_drep
        .as_ref()
        .map(map_drep_choice)
        .transpose()
        .map_err(|e| RESTError::encoding_failed(&format!("dRep: {e}")))?;

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
            )) => Ok(Some(registrations)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account registrations",
            )),
        },
    )
    .await?;

    let Some(registrations) = registrations else {
        return Err(RESTError::not_found("Account not found"));
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
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while resolving transaction hashes",
            )),
        },
    )
    .await?;

    let mut rest_response = Vec::new();

    for r in registrations {
        let Some(tx_hash) = tx_hashes.get(&r.tx_identifier) else {
            return Err(RESTError::InternalServerError(
                "Missing tx hash for registration".to_string(),
            ));
        };

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
            )) => Ok(Some(delegations)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account delegations",
            )),
        },
    )
    .await?;

    let Some(delegations) = delegations else {
        return Err(RESTError::not_found("Account not found"));
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
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while resolving transaction hashes",
            )),
        },
    )
    .await?;

    let mut rest_response = Vec::new();

    for r in delegations {
        let Some(tx_hash) = tx_hashes.get(&r.tx_identifier) else {
            return Err(RESTError::InternalServerError(
                "Missing tx hash for delegation".to_string(),
            ));
        };

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
            )) => Ok(Some(mirs)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account mirs",
            )),
        },
    )
    .await?;

    let Some(mirs) = mirs else {
        return Err(RESTError::not_found("Account not found"));
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
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while resolving transaction hashes",
            )),
        },
    )
    .await?;

    let mut rest_response = Vec::new();

    for r in mirs {
        let Some(tx_hash) = tx_hashes.get(&r.tx_identifier) else {
            return Err(RESTError::InternalServerError(
                "Missing tx hash for MIR record".to_string(),
            ));
        };

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
                AccountsStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account withdrawals",
            )),
        },
    )
    .await?;

    let Some(withdrawals) = withdrawals else {
        return Err(RESTError::not_found("Account not found"));
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
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while resolving transaction hashes",
            )),
        },
    )
    .await?;

    let mut rest_response = Vec::new();

    for w in withdrawals {
        let Some(tx_hash) = tx_hashes.get(&w.tx_identifier) else {
            return Err(RESTError::InternalServerError(
                "Missing tx hash for withdrawal".to_string(),
            ));
        };

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
            )) => Ok(Some(rewards)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account rewards",
            )),
        },
    )
    .await?;

    let Some(rewards) = rewards else {
        return Err(RESTError::not_found("Account not found"));
    };

    let rest_response = rewards
        .iter()
        .map(|r| r.try_into())
        .collect::<Result<Vec<AccountRewardREST>, _>>()
        .map_err(|e| {
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
            )) => Ok(Some(addresses)),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account addresses",
            )),
        },
    )
    .await?;

    let Some(addresses) = addresses else {
        return Err(RESTError::not_found("Account not found"));
    };

    let rest_response = addresses
        .iter()
        .map(|r| {
            Ok(AccountAddressREST {
                address: r
                    .to_string()
                    .map_err(|e| RESTError::InternalServerError(format!("Invalid address: {e}")))?,
            })
        })
        .collect::<Result<Vec<AccountAddressREST>, RESTError>>()?;

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/accounts/{stake_address}/addresses/assets` Blockfrost-compatible endpoint
pub async fn handle_account_assets_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

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
                AccountsStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account addresses",
            )),
        },
    )
    .await?;

    let Some(addresses) = addresses else {
        return Err(RESTError::not_found("Account not found"));
    };

    // Get utxos from address state
    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressesUTxOs { addresses },
    )));
    let utxo_identifiers = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressesUTxOs(utxos),
            )) => Ok(utxos),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account UTxOs",
            )),
        },
    )
    .await?;

    // Get utxo balance sum from utxo state
    let msg = Arc::new(Message::StateQuery(StateQuery::UTxOs(
        UTxOStateQuery::GetUTxOsSum { utxo_identifiers },
    )));
    let utxos_balance = query_state(
        &context,
        &handlers_config.utxos_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::UTxOsSum(sum),
            )) => Ok(sum),
            Message::StateQueryResponse(StateQueryResponse::UTxOs(
                UTxOStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving UTxO sum",
            )),
        },
    )
    .await?;

    let mut rest_response = AmountList::from(utxos_balance).0;
    if !rest_response.is_empty() {
        rest_response.drain(..1);
    }

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/accounts/{stake_address}/addresses/total` Blockfrost-compatible endpoint
pub async fn handle_account_totals_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

    // Get addresses from historical accounts state
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountAssociatedAddresses {
            account: account.clone(),
        },
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
                AccountsStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account addresses",
            )),
        },
    )
    .await?;

    let Some(addresses) = addresses else {
        return Err(RESTError::not_found("Account not found"));
    };

    // Get totals from address state
    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressesTotals { addresses },
    )));
    let totals = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressesTotals(utxos),
            )) => Ok(utxos),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account totals",
            )),
        },
    )
    .await?;

    let rest_response = AccountTotalsREST {
        stake_address: account.to_string()?,
        received_sum: totals.received.into(),
        sent_sum: totals.sent.into(),
        tx_count: totals.tx_count,
    };

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

/// Handle `/accounts/{stake_address}/utxos` Blockfrost-compatible endpoint
pub async fn handle_account_utxos_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse, RESTError> {
    let account = parse_stake_address(&params)?;

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
                AccountsStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account addresses",
            )),
        },
    )
    .await?;

    let Some(addresses) = addresses else {
        return Err(RESTError::not_found("Account not found"));
    };

    // Get utxos from address state
    let msg = Arc::new(Message::StateQuery(StateQuery::Addresses(
        AddressStateQuery::GetAddressesUTxOs { addresses },
    )));
    let utxo_identifiers = query_state(
        &context,
        &handlers_config.addresses_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::AddressesUTxOs(utxos),
            )) => Ok(utxos),
            Message::StateQueryResponse(StateQueryResponse::Addresses(
                AddressStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account UTxOs",
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
                BlocksStateQueryResponse::UTxOHashes(utxos),
            )) => Ok(utxos),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving account UTxOs",
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
        let tx_hash = hashes.tx_hashes.get(i).map(hex::encode).unwrap_or_default();
        let block_hash = hashes.block_hashes.get(i).map(hex::encode).unwrap_or_default();
        let output_index = utxo_identifiers.get(i).map(|id| id.output_index()).unwrap_or(0);
        let (data_hash, inline_datum) = match &entry.datum {
            Some(Datum::Hash(h)) => (Some(hex::encode(h)), None),
            Some(Datum::Inline(bytes)) => (None, Some(hex::encode(bytes))),
            None => (None, None),
        };
        let reference_script_hash = match &entry.reference_script {
            Some(script) => {
                let bytes = match script {
                    ReferenceScript::Native(b)
                    | ReferenceScript::PlutusV1(b)
                    | ReferenceScript::PlutusV2(b)
                    | ReferenceScript::PlutusV3(b) => b,
                };
                let mut hasher = Blake2b512::new();
                hasher.update(bytes);
                let result = hasher.finalize();
                Some(hex::encode(&result[..32]))
            }
            None => None,
        };

        rest_response.push(AccountUTxOREST {
            address: entry.address.to_string()?,
            tx_hash,
            output_index,
            amount: entry.value.into(),
            block: block_hash,
            data_hash,
            inline_datum,
            reference_script_hash,
        })
    }

    let json = serde_json::to_string_pretty(&rest_response)?;
    Ok(RESTResponse::with_json(200, &json))
}

fn parse_stake_address(params: &[String]) -> Result<StakeAddress, RESTError> {
    let Some(stake_key) = params.first() else {
        return Err(RESTError::param_missing("stake address"));
    };

    StakeAddress::from_string(stake_key)
        .map_err(|_| RESTError::invalid_param("stake address", "not a valid stake address"))
}

fn map_drep_choice(drep: &DRepChoice) -> Result<DRepChoiceRest, RESTError> {
    match drep {
        DRepChoice::Key(hash) => {
            let val = hash
                .to_vec()
                .to_bech32_with_hrp("drep")
                .map_err(|e| RESTError::encoding_failed(&format!("DRep Key: {e}")))?;
            Ok(DRepChoiceRest {
                drep_type: "Key".to_string(),
                value: Some(val),
            })
        }
        DRepChoice::Script(hash) => {
            let val = hash
                .to_vec()
                .to_bech32_with_hrp("drep_script")
                .map_err(|e| RESTError::encoding_failed(&format!("DRep Script: {e}")))?;
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
