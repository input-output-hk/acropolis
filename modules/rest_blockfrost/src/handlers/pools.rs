//! REST handlers for Acropolis Blockfrost /pools endpoints
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        epochs::{EpochsStateQuery, EpochsStateQueryResponse},
        parameters::{ParametersStateQuery, ParametersStateQueryResponse},
        pools::{PoolsStateQuery, PoolsStateQueryResponse},
        utils::query_state,
    },
    serialization::Bech32WithHrp,
    PoolRetirement,
};
use anyhow::Result;
use caryatid_sdk::Context;
use rust_decimal::Decimal;
use std::{sync::Arc, time::Duration};

use crate::handlers_config::HandlersConfig;
use crate::types::{PoolEpochStateRest, PoolExtendedRest, PoolMetadataRest, PoolRetirementRest};
use crate::utils::fetch_pool_metadata;

/// Handle `/pools` Blockfrost-compatible endpoint
pub async fn handle_pools_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsList,
    )));

    // Send message via message bus
    let raw = context.message_bus.request(&handlers_config.pools_query_topic, msg).await?;

    // Unwrap and match
    let message = Arc::try_unwrap(raw).unwrap_or_else(|arc| (*arc).clone());

    let pool_operators = match message {
        Message::StateQueryResponse(StateQueryResponse::Pools(
            PoolsStateQueryResponse::PoolsList(pools),
        )) => pools.pool_operators,

        Message::StateQueryResponse(StateQueryResponse::Pools(PoolsStateQueryResponse::Error(
            e,
        ))) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving pools list: {e}"),
            ));
        }

        _ => return Ok(RESTResponse::with_text(500, "Unexpected message type")),
    };

    let pool_ids = pool_operators
        .iter()
        .map(|operator| operator.to_bech32_with_hrp("pool"))
        .collect::<Result<Vec<String>, _>>();

    match pool_ids {
        Ok(pool_ids) => match serde_json::to_string(&pool_ids) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving pools list: {e}"),
            )),
        },
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pools list: {e}"),
        )),
    }
}

/// Handle `/pools/extended` `/pools/retired` `/pools/retiring` `/pools/{pool_id}` Blockfrost-compatible endpoint
pub async fn handle_pools_extended_retired_retiring_single_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    match param.as_str() {
        "extended" => {
            return handle_pools_extended_blockfrost(context.clone(), handlers_config.clone()).await
        }
        "retired" => {
            return handle_pools_retired_blockfrost(context.clone(), handlers_config.clone()).await
        }
        "retiring" => {
            return handle_pools_retiring_blockfrost(context.clone(), handlers_config.clone()).await
        }
        _ => match Vec::<u8>::from_bech32_with_hrp(param, "pool") {
            Ok(pool_id) => return handle_pools_spo_blockfrost(context.clone(), pool_id).await,
            Err(e) => {
                return Ok(RESTResponse::with_text(
                    400,
                    &format!("Invalid Bech32 stake pool ID: {param}. Error: {e}"),
                ));
            }
        },
    }
}

async fn handle_pools_extended_blockfrost(
    context: Arc<Context<Message>>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    // Get pools info from spo-state
    let pools_list_with_info_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsListWithInfo,
    )));
    let pools_list_with_info = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pools_list_with_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolsListWithInfo(pools_list_with_info),
            )) => Ok(pools_list_with_info.pools),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving pools list: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving pools list with info"
                ))
            }
        },
    )
    .await?;

    // if pools are empty, return empty list
    if pools_list_with_info.is_empty() {
        return Ok(RESTResponse::with_json(200, "[]"));
    }

    // Populate pools_operators and pools_vrf_key_hashes
    let pools_operators =
        pools_list_with_info.iter().map(|(pool_operator, _)| pool_operator).collect::<Vec<_>>();
    let pools_vrf_key_hashes = pools_list_with_info
        .iter()
        .map(|(_, pool_registration)| pool_registration.vrf_key_hash.clone())
        .collect::<Vec<_>>();

    // Get Latest Epoch from epochs-state
    let latest_epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch_info = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        latest_epoch_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving latest epoch: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving latest epoch"
                ))
            }
        },
    )
    .await?;
    let latest_epoch = latest_epoch_info.epoch;

    // Get active stake for each pool from spo-state
    let pools_active_stakes_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsActiveStakes {
            pools_operators: pools_operators.iter().map(|&op| op.clone()).collect(),
            epoch: latest_epoch,
        },
    )));
    let (pools_active_stakes, total_active_stake) = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pools_active_stakes_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolsActiveStakes(res),
            )) => Ok((res.active_stakes, res.total_active_stake)),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving pools active stakes: {e}"
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving pools active stakes"
                ))
            }
        },
    )
    .await?;

    // Get live stake for each pool from accounts-state
    let pools_live_stakes_msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetPoolsLiveStakes {
            pools_operators: pools_operators.iter().map(|&op| op.clone()).collect(),
        },
    )));
    let pools_live_stakes = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        pools_live_stakes_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::PoolsLiveStakes(pools_live_stakes),
            )) => Ok(pools_live_stakes.live_stakes),

            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving pools live stakes: {e}"
                ));
            }

            _ => return Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    // Get total blocks minted for each pool from SPO state
    let total_blocks_minted_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsTotalBlocksMinted {
            vrf_key_hashes: pools_vrf_key_hashes.clone(),
        },
    )));
    let total_blocks_minted = query_state(
        &context,
        &handlers_config.pools_query_topic,
        total_blocks_minted_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolsTotalBlocksMinted(res),
            )) => Ok(res.total_blocks_minted),

            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving pools total blocks minted: {e}"
                ));
            }

            _ => return Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    // Get current epoch's blocks minted for each pool from epoch-activity-counter
    let current_blocks_minted_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetBlocksMintedByPools {
            vrf_key_hashes: pools_vrf_key_hashes,
        },
    )));
    let current_blocks_minted = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        current_blocks_minted_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::BlocksMintedByPools(res),
            )) => Ok(res.blocks_minted),

            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving pools current blocks minted: {e}"
                ));
            }

            _ => return Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    let aggregated_blocks_minted = total_blocks_minted
        .iter()
        .zip(current_blocks_minted.iter())
        .map(|(total, current)| total + current)
        .collect::<Vec<_>>();

    // Get latest parameters from parameters-state
    let latest_parameters_msg = Arc::new(Message::StateQuery(StateQuery::Parameters(
        ParametersStateQuery::GetLatestEpochParameters,
    )));
    let latest_parameters = query_state(
        &context,
        &handlers_config.parameters_query_topic,
        latest_parameters_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Parameters(
                ParametersStateQueryResponse::LatestEpochParameters(params),
            )) => Ok(params),
            Message::StateQueryResponse(StateQueryResponse::Parameters(
                ParametersStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving latest parameters: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;
    let Some(stake_pool_target_num) =
        latest_parameters.shelley.map(|shelly| shelly.protocol_params.stake_pool_target_num)
    else {
        // when shelly era is not started, return empty list
        return Ok(RESTResponse::with_json(500, "[]"));
    };

    let pools_extened_rest_results: Result<Vec<PoolExtendedRest>, anyhow::Error> =
        pools_list_with_info
            .iter()
            .enumerate()
            .map(|(i, (pool_operator, pool_registration))| {
                Ok(PoolExtendedRest {
                    pool_id: pool_operator.to_bech32_with_hrp("pool")?,
                    hex: hex::encode(pool_operator),
                    active_stake: pools_active_stakes[i].to_string(),
                    live_stake: pools_live_stakes[i].to_string(),
                    blocks_minted: aggregated_blocks_minted[i],
                    live_saturation: if total_active_stake > 0 {
                        Decimal::from(pools_live_stakes[i]) * Decimal::from(stake_pool_target_num)
                            / Decimal::from(total_active_stake)
                    } else {
                        Decimal::from(0)
                    },
                    declared_pledge: pool_registration.pledge.to_string(),
                    margin_cost: pool_registration.margin.to_f32(),
                    fixed_cost: pool_registration.cost.to_string(),
                })
            })
            .collect();

    match pools_extened_rest_results {
        Ok(pools_extened_rest) => match serde_json::to_string(&pools_extened_rest) {
            Ok(json) => Ok(RESTResponse::with_json(200, &json)),
            Err(e) => Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while extended retrieving pools list: {e}"),
            )),
        },
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while extended retrieving pools list: {e}"),
        )),
    }
}

async fn handle_pools_retired_blockfrost(
    context: Arc<Context<Message>>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    // Get retired pools from spo-state
    let retired_pools_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsRetiredList,
    )));
    let retired_pools = query_state(
        &context,
        &handlers_config.pools_query_topic,
        retired_pools_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolsRetiredList(retired_pools),
            )) => Ok(retired_pools.retired_pools),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving retired pools: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    let retired_pools_rest = retired_pools
        .iter()
        .filter_map(|PoolRetirement { operator, epoch }| {
            let pool_id = operator.to_bech32_with_hrp("pool").ok()?;
            Some(PoolRetirementRest {
                pool_id,
                epoch: *epoch,
            })
        })
        .collect::<Vec<_>>();

    match serde_json::to_string(&retired_pools_rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving retired pools: {e}"),
        )),
    }
}

async fn handle_pools_retiring_blockfrost(
    context: Arc<Context<Message>>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    // Get retiring pools from spo-state
    let retiring_pools_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsRetiringList,
    )));
    let retiring_pools = query_state(
        &context,
        &handlers_config.pools_query_topic,
        retiring_pools_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolsRetiringList(retiring_pools),
            )) => Ok(retiring_pools.retiring_pools),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving retiring pools: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    let retiring_pools_rest = retiring_pools
        .iter()
        .filter_map(|PoolRetirement { operator, epoch }| {
            let pool_id = operator.to_bech32_with_hrp("pool").ok()?;
            Some(PoolRetirementRest {
                pool_id,
                epoch: *epoch,
            })
        })
        .collect::<Vec<_>>();

    match serde_json::to_string(&retiring_pools_rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving retiring pools: {e}"),
        )),
    }
}

async fn handle_pools_spo_blockfrost(
    _context: Arc<Context<Message>>,
    _pool_operator: Vec<u8>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_history_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(pool_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing pool ID parameter"));
    };

    let Ok(spo) = Vec::<u8>::from_bech32_with_hrp(pool_id, "pool") else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 stake pool ID: {pool_id}"),
        ));
    };

    // get latest epoch from epochs-state
    let latest_epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch_info = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        latest_epoch_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving latest epoch: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;
    let latest_epoch = latest_epoch_info.epoch;

    let pool_history_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolHistory { pool_id: spo },
    )));
    let mut pool_history: Vec<PoolEpochStateRest> = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pool_history_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolHistory(pool_history),
            )) => Ok(pool_history.history.into_iter().map(|state| state.into()).collect()),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving pool history: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    // remove epoch state whose epoch is greater than or equal to latest_epoch
    pool_history.retain(|state| state.epoch < latest_epoch);

    match serde_json::to_string(&pool_history) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pool history: {e}"),
        )),
    }
}

pub async fn handle_pool_metadata_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(pool_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing pool ID parameter"));
    };

    let Ok(spo) = Vec::<u8>::from_bech32_with_hrp(pool_id, "pool") else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 stake pool ID: {pool_id}"),
        ));
    };

    let pool_metadata_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolMetadata {
            pool_id: spo.clone(),
        },
    )));
    let pool_metadata = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pool_metadata_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolMetadata(pool_metadata),
            )) => Ok(pool_metadata),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!("Not found")),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving pool metadata: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    let pool_metadata_json = fetch_pool_metadata(
        pool_metadata.url.clone(),
        Duration::from_secs(handlers_config.external_api_timeout),
    )
    .await?;
    let pool_metadata_rest = PoolMetadataRest {
        pool_id: pool_id.to_string(),
        hex: hex::encode(spo),
        url: pool_metadata.url,
        hash: hex::encode(pool_metadata.hash),
        ticker: pool_metadata_json.ticker,
        name: pool_metadata_json.name,
        description: pool_metadata_json.description,
        homepage: pool_metadata_json.homepage,
    };

    match serde_json::to_string(&pool_metadata_rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pool metadata: {e}"),
        )),
    }
}

pub async fn handle_pool_relays_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_delegators_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_updates_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_votes_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}
