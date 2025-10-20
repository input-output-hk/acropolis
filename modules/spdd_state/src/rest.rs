use crate::state::State;
use acropolis_common::serialization::Bech32WithHrp;
use acropolis_common::{extract_strict_query_params, messages::RESTResponse};
use anyhow::Result;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

/// Handles /spdd
pub async fn handle_spdd(
    state: Arc<Mutex<State>>,
    params: HashMap<String, String>,
) -> Result<RESTResponse> {
    let locked = state.lock().await;

    extract_strict_query_params!(params, {
        "epoch" => epoch: Option<u64>,
    });

    let spdd_opt = match epoch {
        Some(epoch) => match locked.get_epoch(epoch) {
            Some(spdd) => Some(spdd),
            None => {
                return Ok(RESTResponse::with_text(
                    404,
                    &format!("SPDD not found for epoch {}", epoch),
                ));
            }
        },
        None => locked.get_latest(),
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
