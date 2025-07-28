//! REST handlers for Acropolis SPO State module
use crate::state::State;
use acropolis_common::{messages::RESTResponse, serialization::Bech32WithHrp};
use acropolis_common::{PoolMetadata, Relay};
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// REST response structure mapping pool ID to its parameters
#[derive(Serialize)]
pub struct PoolParamsRest {
    pub margin: f64,
    pub pledge: u64,
    pub fixed_cost: u64,
}

/// REST response structure for single pool
#[derive(Serialize)]
pub struct PoolInfoRest {
    pub vrf_key_hash: String,
    pub pledge: u64,
    pub cost: u64,
    pub margin: f64,
    pub reward_account: String,
    pub pool_owners: Vec<String>,
    pub relays: Vec<Relay>,
    pub pool_metadata: Option<PoolMetadata>,
}

/// REST response structure for retiring pools
#[derive(Serialize)]
pub struct PoolRetirementRest {
    pub pool_id: String,
    pub epoch: u64,
}

/// Handles /pools
pub async fn handle_list(state: Arc<Mutex<State>>) -> Result<RESTResponse> {
    let locked = state.lock().await;

    match locked.list_pools_with_info() {
        Some(pools) => {
            let mut response: HashMap<String, PoolParamsRest> = HashMap::new();

            for (operator, reg) in pools {
                let operator_id = match operator.to_bech32_with_hrp("pool") {
                    Ok(val) => val,
                    Err(e) => {
                        return Ok(RESTResponse::with_text(
                            500,
                            &format!("Internal server error retrieving pools list: {e}"),
                        ));
                    }
                };

                let margin = if reg.margin.denominator == 0 {
                    0.0
                } else {
                    reg.margin.numerator as f64 / reg.margin.denominator as f64
                };

                response.insert(
                    operator_id,
                    PoolParamsRest {
                        margin,
                        pledge: reg.pledge,
                        fixed_cost: reg.cost,
                    },
                );
            }

            match serde_json::to_string(&response) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while retrieving pools list: {e}"),
                )),
            }
        }
        None => Ok(RESTResponse::with_text(200, "{}")),
    }
}

/// Handles /pools/{pool_id}
/// Handles /pools/retiring
pub async fn handle_spo(state: Arc<Mutex<State>>, param: String) -> Result<RESTResponse> {
    // check if param is "retiring"
    if param == "retiring" {
        return handle_retiring_pools(state).await;
    }

    let pool_id = match Vec::<u8>::from_bech32_with_hrp(&param, "pool") {
        Ok(id) => id,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                400,
                &format!("Invalid Bech32 stake pool ID: {param}. Error: {e}"),
            ));
        }
    };

    let locked = state.lock().await;
    match locked.get(&pool_id) {
        Some(reg) => {
            let margin = if reg.margin.denominator == 0 {
                0.0
            } else {
                reg.margin.numerator as f64 / reg.margin.denominator as f64
            };

            let reward_account = match reg.reward_account.to_bech32_with_hrp("stake") {
                Ok(val) => val,
                Err(e) => {
                    return Ok(RESTResponse::with_text(
                        500,
                        &format!("Internal server error retrieving stake pool information: {e}"),
                    ));
                }
            };

            let mut pool_owners = Vec::new();
            for owner in &reg.pool_owners {
                match owner.to_bech32_with_hrp("stake") {
                    Ok(val) => pool_owners.push(val),
                    Err(e) => {
                        return Ok(RESTResponse::with_text(
                            500,
                            &format!(
                                "Internal server error retrieving stake pool information: {e}"
                            ),
                        ));
                    }
                }
            }

            let response = PoolInfoRest {
                vrf_key_hash: hex::encode(&reg.vrf_key_hash),
                pledge: reg.pledge,
                cost: reg.cost,
                margin,
                reward_account,
                pool_owners,
                relays: reg.relays.clone(),
                pool_metadata: reg.pool_metadata.clone(),
            };

            match serde_json::to_string(&response) {
                Ok(body) => Ok(RESTResponse::with_json(200, &body)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error retrieving stake pool information: {e}"),
                )),
            }
        }
        None => Ok(RESTResponse::with_text(404, "Stake pool not found")),
    }
}

/// Handles /pools/retiring
async fn handle_retiring_pools(state: Arc<Mutex<State>>) -> Result<RESTResponse> {
    let locked = state.lock().await;
    let mut response: Vec<PoolRetirementRest> = Vec::new();
    for retiring_pool in locked.get_retiring_pools() {
        let pool_id = match retiring_pool.operator.to_bech32_with_hrp("pool") {
            Ok(val) => val,
            Err(e) => {
                return Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error retrieving retiring pools: {e}"),
                ));
            }
        };
        response.push(PoolRetirementRest {
            pool_id,
            epoch: retiring_pool.epoch,
        });
    }

    match serde_json::to_string(&response) {
        Ok(body) => Ok(RESTResponse::with_json(200, &body)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error retrieving retiring pools: {e}"),
        )),
    }
}
