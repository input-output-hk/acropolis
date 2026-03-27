use crate::state::State;
use acropolis_common::rest_error::RESTError;
use acropolis_common::serialization::Bech32Conversion;
use acropolis_common::state_history::StateHistory;
use acropolis_common::DelegatedStake;
use acropolis_common::{extract_strict_query_params, messages::RESTResponse};
use anyhow::Result;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

/// Handles /spdd
pub async fn handle_spdd(
    history: Arc<Mutex<StateHistory<State>>>,
    params: HashMap<String, String>,
) -> Result<RESTResponse, RESTError> {
    let locked = history.lock().await;
    let state = {
        match locked.current() {
            Some(state) => state,
            None => {
                return Ok(RESTResponse::with_text(
                    404,
                    "SPDD state not yet initialized",
                ))
            }
        }
    };

    extract_strict_query_params!(params, {
        "epoch" => epoch: Option<u64>,
    });

    let spdd = match epoch {
        Some(epoch) => match locked.get_by_index(epoch + 1) {
            Some(epoch_state) => epoch_state.get_latest(),
            None => {
                return Ok(RESTResponse::with_text(
                    404,
                    &format!("SPDD not found for epoch {}", epoch),
                ));
            }
        },
        None => state.get_latest(),
    };

    let spdd: HashMap<String, DelegatedStake> =
        spdd.iter().map(|(k, v)| (k.to_bech32().unwrap_or_else(|_| hex::encode(k)), *v)).collect();

    match serde_json::to_string(&spdd) {
        Ok(body) => Ok(RESTResponse::with_json(200, &body)),
        Err(e) => Err(RESTError::from(e)),
    }
}
