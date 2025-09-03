use crate::{epochs_history::EpochsHistoryState, state::State};
use acropolis_common::{messages::RESTResponse, state_history::StateHistory};
use anyhow::Result;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Serialize)]
pub struct EpochActivityRest {
    pub epoch: u64,
    pub total_blocks: usize,
    pub total_fees: u64,
    pub vrf_vkey_hashes: Vec<VRFKeyCount>,
}
#[derive(Serialize)]
pub struct VRFKeyCount {
    pub vrf_key_hash: String,
    pub block_count: usize,
}

/// Handles /epoch
pub async fn handle_epoch(history: Arc<Mutex<StateHistory<State>>>) -> Result<RESTResponse> {
    let state = history.lock().await.get_current_state();
    let epoch_data = state.get_epoch_activity_message();

    let response = EpochActivityRest {
        epoch: epoch_data.epoch,
        total_blocks: epoch_data.total_blocks,
        total_fees: epoch_data.total_fees,
        vrf_vkey_hashes: epoch_data
            .vrf_vkey_hashes
            .iter()
            .map(|(key, count)| VRFKeyCount {
                vrf_key_hash: hex::encode(key),
                block_count: *count,
            })
            .collect(),
    };

    let json = match serde_json::to_string(&response) {
        Ok(j) => j,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving current epoch: {e}"),
            ));
        }
    };
    Ok(RESTResponse::with_json(200, &json))
}

/// Handles /epochs/{epoch}
pub async fn handle_historical_epoch(
    epochs_history: EpochsHistoryState,
    epoch: String,
) -> Result<RESTResponse> {
    let parsed_epoch = match epoch.parse::<u64>() {
        Ok(v) => v,
        Err(_) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid epoch number: {epoch}"),
            ))
        }
    };

    match epochs_history.get_historical_epoch(parsed_epoch) {
        Err(_) => Ok(RESTResponse::with_text(
            501,
            "Historical epoch storage not enabled",
        )),
        Ok(Some(epoch_data)) => {
            let response = EpochActivityRest {
                epoch: epoch_data.epoch,
                total_blocks: epoch_data.total_blocks,
                total_fees: epoch_data.total_fees,
                vrf_vkey_hashes: epoch_data
                    .vrf_vkey_hashes
                    .iter()
                    .map(|(key, count)| VRFKeyCount {
                        vrf_key_hash: hex::encode(key),
                        block_count: *count,
                    })
                    .collect(),
            };
            let json = match serde_json::to_string(&response) {
                Ok(j) => j,
                Err(e) => {
                    return Ok(RESTResponse::with_text(
                        500,
                        &format!(
                            "Internal server error while retrieving epoch {parsed_epoch}: {e}"
                        ),
                    ));
                }
            };
            Ok(RESTResponse::with_json(200, &json))
        }
        Ok(None) => Ok(RESTResponse::with_text(
            404,
            &format!("Epoch {parsed_epoch} not found"),
        )),
    }
}
