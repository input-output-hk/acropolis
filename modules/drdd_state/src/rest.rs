use crate::state::State;
use acropolis_common::{extract_strict_query_params, messages::RESTResponse, DRepCredential};
use anyhow::Result;
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
    state: Option<Arc<Mutex<State>>>,
    params: HashMap<String, String>,
) -> Result<RESTResponse> {
    let locked = match state.as_ref() {
        Some(state) => state.lock().await,
        None => {
            return Ok(RESTResponse::with_text(
                503,
                "DRDD storage is disabled by configuration",
            ));
        }
    };

    extract_strict_query_params!(params, {
        "epoch" => epoch: Option<u64>,
    });

    let drdd_opt = match epoch {
        Some(epoch) => match locked.get_epoch(epoch) {
            Some(drdd) => Some(drdd),
            None => {
                return Ok(RESTResponse::with_text(
                    404,
                    &format!("DRDD not found for epoch {}", epoch),
                ));
            }
        },
        None => locked.get_latest(),
    };

    if let Some(drdd) = drdd_opt {
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

        match serde_json::to_string(&response) {
            Ok(body) => Ok(RESTResponse::with_json(200, &body)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error retrieving DRep delegation distribution: {e}"),
            )),
        }
    } else {
        let response = DRDDResponse {
            dreps: HashMap::new(),
            abstain: 0,
            no_confidence: 0,
        };

        match serde_json::to_string(&response) {
            Ok(body) => Ok(RESTResponse::with_json(200, &body)),
            Err(_) => Ok(RESTResponse::with_text(
                500,
                "Internal server error serializing empty DRDD response",
            )),
        }
    }
}
