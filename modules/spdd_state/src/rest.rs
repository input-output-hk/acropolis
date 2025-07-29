use crate::state::State;
use acropolis_common::messages::RESTResponse;
use acropolis_common::serialization::Bech32WithHrp;
use anyhow::Result;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

/// Handles /spdd
pub async fn handle_spdd(state: Arc<Mutex<State>>, params: Vec<String>) -> Result<RESTResponse> {
    let locked = state.lock().await;

    let spdd_opt = if params.len() == 1 {
        match params[0].parse::<u64>() {
            Ok(epoch) => locked.get_epoch(epoch),
            Err(_) => {
                return Ok(RESTResponse::with_text(
                    400,
                    "Invalid epoch query parameter: must be a number",
                ));
            }
        }
    } else {
        locked.get_latest()
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
