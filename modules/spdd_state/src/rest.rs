use crate::state::State;
use acropolis_common::rest_error::RESTError;
use acropolis_common::serialization::Bech32Conversion;
use acropolis_common::DelegatedStake;
use acropolis_common::{extract_strict_query_params, messages::RESTResponse};
use anyhow::Result;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

/// Handles /spdd
pub async fn handle_spdd(
    state: Arc<Mutex<State>>,
    params: HashMap<String, String>,
) -> Result<RESTResponse, RESTError> {
    let locked = state.lock().await;

    extract_strict_query_params!(params, {
        "epoch" => epoch: Option<u64>,
    });

    let spdd_opt = match epoch {
        Some(epoch) => match locked.get_epoch(epoch) {
            Some(spdd) => Some(spdd),
            None => {
                return Err(RESTError::not_found(&format!("SPDD for epoch {}", epoch)));
            }
        },
        None => locked.get_latest(),
    };

    if let Some(spdd) = spdd_opt {
        let spdd: HashMap<String, DelegatedStake> = spdd
            .iter()
            .map(|(k, v)| (k.to_bech32().unwrap_or_else(|_| hex::encode(k)), *v))
            .collect();

        let body =
            serde_json::to_string(&spdd).map_err(|e| RESTError::serialization_failed("SPDD", e))?;

        Ok(RESTResponse::with_json(200, &body))
    } else {
        Ok(RESTResponse::with_json(200, "{}"))
    }
}
