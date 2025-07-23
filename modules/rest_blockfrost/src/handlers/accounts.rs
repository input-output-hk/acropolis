//! REST handlers for Acropolis Blockfrost /accounts endpoints
use std::sync::Arc;

use acropolis_common::messages::{Message, RESTResponse, StateQuery, StateQueryResponse};
use acropolis_common::serialization::Bech32WithHrp;
use acropolis_common::{Address, DRepChoice, StakeAddress, StakeAddressPayload};
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
    param: String,
) -> Result<RESTResponse> {
    // Convert Bech32 stake address to StakeCredential
    let stake_key = match Address::from_string(&param) {
        Ok(Address::Stake(StakeAddress {
            payload: StakeAddressPayload::StakeKeyHash(hash),
            ..
        })) => hash,
        _ => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Not a valid stake address: {param}"),
            ));
        }
    };

    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::GetAccountInfo {
        stake_key,
    }));

    // Send message via message bus
    let raw = context.message_bus.request("accounts-state", msg).await?;

    // Unwrap and match
    let message = Arc::try_unwrap(raw).unwrap_or_else(|arc| (*arc).clone());

    let account = match message {
        Message::StateQueryResponse(StateQueryResponse::AccountInfo(account)) => account,
        Message::StateQueryResponse(StateQueryResponse::NotFound) => {
            return Ok(RESTResponse::with_text(404, "Stake address not found"));
        }
        Message::StateQueryResponse(StateQueryResponse::Error(e)) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error: {e}"),
            ));
        }
        _ => return Ok(RESTResponse::with_text(500, "Unexpected message type")),
    };

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
        delegated_spo: delegated_spo,
        delegated_drep: delegated_drep,
    };

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
