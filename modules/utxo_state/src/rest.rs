//! REST handlers for Acropolis UTxO State module

use std::sync::Arc;

use crate::state::{State, UTXOKey};
use acropolis_common::messages::RESTResponse;
use anyhow::Result;
use tokio::sync::Mutex;

// Handles /utxo/<tx_hash:index>
pub async fn handle_single_utxo(
    state: Arc<Mutex<State>>,
    param: String,
) -> Result<RESTResponse, anyhow::Error> {
    let (tx_hash_str, index_str) = match param.split_once(':') {
        Some((tx, idx)) => (tx, idx),
        None => {
            return Ok(RESTResponse::with_text(
                400,
                &format!(
                    "Parameter must be in <tx_hash>:<index> format. Provided param: {}",
                    param
                ),
            ));
        }
    };

    let tx_hash_bytes = match hex::decode(tx_hash_str) {
        Ok(bytes) => bytes,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid tx_hash: {e}"),
            ));
        }
    };

    let index = match index_str.parse::<u64>() {
        Ok(idx) => idx,
        Err(e) => {
            return Ok(RESTResponse::with_text(400, &format!("Invalid index: {e}")));
        }
    };

    let locked = state.lock().await;
    let key = UTXOKey::new(&tx_hash_bytes, index);

    let utxo_opt = match locked.lookup_utxo(&key).await {
        Ok(res) => res,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving UTxO: {e}"),
            ));
        }
    };

    match utxo_opt {
        Some(utxo) => {
            let address_text = match utxo.address.to_string() {
                Ok(addr) => addr,
                Err(e) => {
                    return Ok(RESTResponse::with_text(
                        500,
                        &format!("Internal server error while retrieving UTxO: {e}"),
                    ));
                }
            };

            let json_response = serde_json::json!({
                "address": address_text,
                "value": utxo.value,
            });

            match serde_json::to_string(&json_response) {
                Ok(body) => Ok(RESTResponse::with_json(200, &body)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while retrieving UTxO: {e}"),
                )),
            }
        }
        None => Ok(RESTResponse::with_text(
            404,
            &format!("UTxO not found. Provided UTxO: {}", param),
        )),
    }
}
