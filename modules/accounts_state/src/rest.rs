//! REST handlers for Acropolis Accounts State module
use crate::state::State;
use acropolis_common::state_history::StateHistory;
use acropolis_common::{messages::RESTResponse, Lovelace};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

/// REST response structure for /accounts/{stake_address}
#[derive(serde::Serialize)]
pub struct APIStakeAccount {
    pub utxo_value: u64,
    pub rewards: u64,
    pub delegated_spo: Option<String>,
    pub delegated_drep: Option<APIDRepChoice>,
}
#[derive(serde::Serialize)]
pub struct APIDRepChoice {
    pub drep_type: String,
    pub value: Option<String>,
}

/// Handles /drdd
#[derive(serde::Serialize, serde::Deserialize)]
struct APIDRepDelegationDistribution {
    pub abstain: Lovelace,
    pub no_confidence: Lovelace,
    pub dreps: Vec<(String, u64)>,
}

/// Handles /pots
pub async fn handle_pots(history: Arc<Mutex<StateHistory<State>>>) -> Result<RESTResponse> {
    let locked = history.lock().await;
    let state = match locked.current() {
        Some(state) => state,
        None => return Ok(RESTResponse::with_json(200, "{}")),
    };

    let pots = state.get_pots();

    match serde_json::to_string(&pots) {
        Ok(body) => Ok(RESTResponse::with_json(200, &body)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pots: {e}"),
        )),
    }
}
