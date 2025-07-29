//! REST handlers for Acropolis Accounts State module
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::state::State;
use acropolis_common::state_history::StateHistory;
use acropolis_common::{messages::RESTResponse, Lovelace};

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

/// Handles /drdd
pub async fn handle_drdd(history: Arc<Mutex<StateHistory<State>>>) -> Result<RESTResponse> {
    let locked = history.lock().await;
    let state = match locked.current() {
        Some(state) => state,
        None => return Ok(RESTResponse::with_json(200, "{}")),
    };

    let drdd = state.generate_drdd();

    let dreps = {
        let mut dreps = Vec::with_capacity(drdd.dreps.len());
        for (cred, amount) in drdd.dreps {
            let bech32 = match cred.to_drep_bech32() {
                Ok(val) => val,
                Err(e) => {
                    return Ok(RESTResponse::with_text(
                        500,
                        &format!("Internal server error while retrieving DRep delegation distribution: {e}"),
                    ));
                }
            };
            dreps.push((bech32, amount));
        }
        dreps
    };

    let response = APIDRepDelegationDistribution {
        abstain: drdd.abstain,
        no_confidence: drdd.no_confidence,
        dreps,
    };

    match serde_json::to_string(&response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving DRep delegation distribution: {e}"),
        )),
    }
}
