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
use crate::types::{AccountWithdrawalREST, DelegationUpdateREST, RegistrationUpdateREST};

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
    let stake_address = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
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

    match serde_json::to_string_pretty(&rest_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving DRep delegation distribution: {e}"),
        )),
    }
}

/// Handle `/accounts/{stake_address}/registrations` Blockfrost-compatible endpoint
pub async fn handle_account_registrations_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let stake_address = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountRegistrationHistory {
            account: stake_address,
        },
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
    let tx_ids: Vec<_> = registrations.iter().map(|r| r.tx_identifier.clone()).collect();
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
            &format!("Internal server error while serializing account registration history: {e}"),
        )),
    }
}

/// Handle `/accounts/{stake_address}/delegations` Blockfrost-compatible endpoint
pub async fn handle_account_delegations_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let stake_address = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountDelegationHistory {
            account: stake_address,
        },
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
    let tx_ids: Vec<_> = delegations.iter().map(|r| r.tx_identifier.clone()).collect();
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

        let pool_id = match r.pool.to_bech32_with_hrp("pool") {
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
            &format!("Internal server error while serializing account delegation history: {e}"),
        )),
    }
}

/// Handle `/accounts/{stake_address}/mirs` Blockfrost-compatible endpoint
pub async fn handle_account_mirs_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let stake_address = match parse_stake_address(&params) {
        Ok(addr) => addr,
        Err(resp) => return Ok(resp),
    };

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountMIRHistory {
            account: stake_address,
        },
    )));

    // Get delegations from historical accounts state
    let mirs = query_state(
        &context,
        &handlers_config.historical_accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountMIRHistory(mirs),
            )) => Ok(mirs),
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
    let tx_ids: Vec<_> = mirs.iter().map(|r| r.tx_identifier.clone()).collect();
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
