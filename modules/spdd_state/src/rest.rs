use crate::state::State;
use acropolis_common::messages::RESTResponse;
use acropolis_common::serialization::Bech32WithHrp;
use anyhow::Result;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

/// Handles /spdd
pub async fn handle_spdd(
    state: Option<Arc<Mutex<State>>>,
    params: HashMap<String, String>,
) -> Result<RESTResponse> {
    let locked = match state.as_ref() {
        Some(state) => state.lock().await,
        None => {
            return Ok(RESTResponse::with_text(
                503,
                "SPDD storage is disabled by configuration",
            ));
        }
    };

    let spdd_opt = if let Some(epoch_str) = params.get("epoch") {
        if params.len() > 1 {
            return Ok(RESTResponse::with_text(
                400,
                "Only 'epoch' is a valid query parameter",
            ));
        }

        match epoch_str.parse::<u64>() {
            Ok(epoch) => match locked.get_epoch(epoch) {
                Some(spdd) => Some(spdd),
                None => {
                    return Ok(RESTResponse::with_text(
                        404,
                        &format!("SPDD not found for epoch {}", epoch),
                    ));
                }
            },
            Err(_) => {
                return Ok(RESTResponse::with_text(
                    400,
                    "Invalid epoch query parameter: must be a number",
                ));
            }
        }
    } else if params.is_empty() {
        locked.get_latest()
    } else {
        return Ok(RESTResponse::with_text(
            400,
            "Unexpected query parameter: only 'epoch' is allowed",
        ));
    };

    if let Some(spdd) = spdd_opt {
        let spdd: HashMap<_, _> = spdd
            .iter()
            .map(|(k, v)| {
                (
                    k.to_bech32_with_hrp("pool").unwrap_or_else(|_| hex::encode(k)),
                    *v,
                )
            })
            .collect();

        match serde_json::to_string(&spdd) {
            Ok(body) => Ok(RESTResponse::with_json(200, &body)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!(
                    "Internal server error retrieving stake pool delegation distribution: {e}"
                ),
            )),
        }
    } else {
        Ok(RESTResponse::with_json(200, "{}"))
    }
}
