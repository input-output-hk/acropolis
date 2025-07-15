//! REST handlers for Acropolis DRep State module

use std::sync::Arc;

use acropolis_common::{messages::RESTResponse, Credential};
use anyhow::Result;
use tokio::sync::Mutex;

use crate::state::State;

// Handle REST requests for /dreps/list
pub async fn handle_list(state: Arc<Mutex<State>>) -> RESTResponse {
    let locked = state.lock().await;

    let drep_list_bech32 =
        locked.list().iter().map(|cred| cred.to_drep_bech32()).collect::<Result<Vec<_>, _>>();

    match drep_list_bech32 {
        Ok(list) => match serde_json::to_string(&list) {
            Ok(json) => RESTResponse::with_json(200, &json),
            Err(e) => RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving DReps: {e}"),
            ),
        },
        Err(e) => RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving DReps: {e}"),
        ),
    }
}

// Handle REST requests for /dreps/<Bech32_DRepCredential>
pub async fn handle_drep(state: Arc<Mutex<State>>, cred_str: String) -> Result<RESTResponse> {
    let cred = match Credential::from_drep_bech32(&cred_str) {
        Ok(c) => c,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid Bech32 DRep ID: {cred_str}. Error: {e}"),
            ));
        }
    };

    let locked = state.lock().await;
    match locked.get_drep(&cred) {
        Some(drep_record) => match serde_json::to_string(drep_record) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving DRep: {e}"),
            )),
        },
        None => Ok(RESTResponse::with_text(404, "DRep not found")),
    }
}
