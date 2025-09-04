//! REST handlers for Acropolis UTxO State module

use std::{collections::HashMap, sync::Arc};

use crate::state::{State, UTXOKey};
use acropolis_common::{messages::RESTResponse, NativeAssets};
use anyhow::Result;
use tokio::sync::Mutex;

/// REST response structure for single UTxO balance
#[derive(serde::Serialize)]
pub struct UTxOBalanceRest {
    pub address: String,
    pub lovelace: u64,
    pub assets: AssetsREST,
}

pub type AssetsREST = HashMap<String, HashMap<String, u64>>;

/// Handles /utxos/{tx_hash:index}
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

            let assets = convert_assets(&utxo.value.assets);

            let response = UTxOBalanceRest {
                address: address_text,
                lovelace: utxo.value.lovelace,
                assets,
            };

            match serde_json::to_string(&response) {
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

fn convert_assets(assets: &NativeAssets) -> AssetsREST {
    let mut rest: AssetsREST = HashMap::new();

    for (policy_id, native_assets) in assets {
        let policy_hex = hex::encode(policy_id);
        let entry = rest.entry(policy_hex).or_default();

        for na in native_assets {
            let name = String::from_utf8(na.name.clone()).unwrap_or_else(|_| hex::encode(&na.name));

            entry.insert(name, na.amount);
        }
    }

    rest
}
