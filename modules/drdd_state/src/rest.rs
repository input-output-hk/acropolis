use crate::state::State;
use acropolis_common::rest_error::RESTError;
use acropolis_common::state_history::StateHistory;
use acropolis_common::{extract_strict_query_params, messages::RESTResponse, DRepCredential};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

// Response struct for DRDD
#[derive(Serialize)]
struct DRDDResponse {
    dreps: HashMap<String, u64>,
    abstain: u64,
    no_confidence: u64,
}

/// Handles /drdd
pub async fn handle_drdd(
    history: Option<Arc<Mutex<StateHistory<State>>>>,
    params: HashMap<String, String>,
) -> Result<RESTResponse, RESTError> {
    let history = match history {
        Some(history) => history,
        None => return Err(RESTError::storage_disabled("DRDD")),
    };

    let locked = history.lock().await;

    let state = locked.current().ok_or_else(|| RESTError::storage_disabled("DRDD"))?;

    extract_strict_query_params!(params, {
        "epoch" => epoch: Option<u64>,
    });

    let drdd = match epoch {
        Some(epoch) => match locked.get_by_index(epoch) {
            Some(epoch_state) => epoch_state.get_latest(),
            None => {
                return Err(RESTError::not_found(&format!("DRDD in epoch {}", epoch)));
            }
        },
        None => state.get_latest(),
    };

    let dreps: HashMap<String, u64> = drdd
        .dreps
        .iter()
        .map(|(k, v)| {
            let key = k.to_drep_bech32().unwrap_or_else(|_| match k {
                DRepCredential::AddrKeyHash(bytes) | DRepCredential::ScriptHash(bytes) => {
                    hex::encode(bytes)
                }
            });
            (key, *v)
        })
        .collect();

    let response = DRDDResponse {
        dreps,
        abstain: drdd.abstain,
        no_confidence: drdd.no_confidence,
    };

    let body = serde_json::to_string(&response)?;
    Ok(RESTResponse::with_json(200, &body))
}
