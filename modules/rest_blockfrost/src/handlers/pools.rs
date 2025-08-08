//! REST handlers for Acropolis Blockfrost /pools endpoints
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        epochs::{EpochsStateQuery, EpochsStateQueryResponse},
        pools::{PoolsStateQuery, PoolsStateQueryResponse},
        utils::query_state,
    },
    serialization::Bech32WithHrp,
};
use anyhow::Result;
use caryatid_sdk::Context;
use rust_decimal::Decimal;
use std::sync::Arc;

use crate::types::{PoolExtendedRest, PoolMetadataRest};

const ACCOUNTS_STATE_TOPIC: &str = "accounts-state";
const POOLS_STATE_TOPIC: &str = "pools-state";
const EPOCH_STATE_TOPIC: &str = "epoch-state";
const PARAMETERS_STATE_TOPIC: &str = "parameters-state";

/// Handle `/pools` Blockfrost-compatible endpoint
pub async fn handle_pools_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    // Prepare the message
    let msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsList,
    )));

    // Send message via message bus
    let raw = context.message_bus.request(POOLS_STATE_TOPIC, msg).await?;

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
) -> Result<RESTResponse> {
    let param = match params.as_slice() {
        [param] => param,
        _ => return Ok(RESTResponse::with_text(400, "Invalid parameters")),
    };

    match param.as_str() {
        "extended" => return handle_pools_extended_blockfrost(context.clone()).await,
        "retired" => return handle_pools_retired_blockfrost(context.clone()).await,
        "retiring" => return handle_pools_retiring_blockfrost(context.clone()).await,
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

async fn handle_pools_extended_blockfrost(context: Arc<Context<Message>>) -> Result<RESTResponse> {
    // Get pools info from spo-state
    let pools_list_with_info_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsListWithInfo,
    )));
    let pools_list_with_info = query_state(
        &context,
        POOLS_STATE_TOPIC,
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
            _ => return Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;
    let pools_operators =
        pools_list_with_info.iter().map(|(pool_operator, _)| pool_operator).collect::<Vec<_>>();
    let pools_vrf_key_hashes = pools_list_with_info
        .iter()
        .map(|(_, pool_registration)| pool_registration.vrf_key_hash.clone())
        .collect::<Vec<_>>();

    // Get active stake for each pool from spo-state
    let pools_active_stakes_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsActiveStakes {
            pools_operators: pools_operators.iter().map(|&op| op.clone()).collect(),
        },
    )));
    let (pools_active_stakes, total_active_stake) = query_state(
        &context,
        POOLS_STATE_TOPIC,
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
            _ => return Err(anyhow::anyhow!("Unexpected message type")),
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
        ACCOUNTS_STATE_TOPIC,
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

    // Get blocks minted for each pool from epoch-activity-counter
    let pools_blocks_minted_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetBlocksMintedByPools {
            vrf_key_hashes: pools_vrf_key_hashes,
        },
    )));
    let pools_blocks_minted = query_state(
        &context,
        EPOCH_STATE_TOPIC,
        pools_blocks_minted_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::BlocksMintedByPools(res),
            )) => Ok(res.blocks_minted),

            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving pools blocks minted: {e}"
                ));
            }

            _ => return Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    // Get latest parameters from parameters-state
    let latest_parameters_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpochParameters,
    )));
    let latest_parameters = query_state(
        &context,
        PARAMETERS_STATE_TOPIC,
        latest_parameters_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpochParameters(res),
            )) => Ok(res.parameters),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
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
        return Ok(RESTResponse::with_text(
            500,
            "Internal server error while retrieving latest parameters: stake_pool_target_num not found",
        ));
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
                    blocks_minted: pools_blocks_minted[i],
                    live_saturation: if total_active_stake > 0 {
                        Decimal::from(pools_live_stakes[i])
                            * Decimal::from(stake_pool_target_num)
                            / Decimal::from(total_active_stake)
                    } else {
                        Decimal::from(0)
                    },
                    declared_pledge: pool_registration.pledge.to_string(),
                    margin_cost: pool_registration.margin.to_f32(),
                    fixed_cost: pool_registration.cost.to_string(),
                    metadata: pool_registration.pool_metadata.as_ref().map(|metadata| {
                        PoolMetadataRest {
                            url: metadata.url.clone(),
                            hash: hex::encode(metadata.hash.clone()),
                            ticker: "ticker".to_string(),
                            name: "name".to_string(),
                            description: "description".to_string(),
                            homepage: "homepage".to_string(),
                        }
                    }),
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

async fn handle_pools_retired_blockfrost(context: Arc<Context<Message>>) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

async fn handle_pools_retiring_blockfrost(context: Arc<Context<Message>>) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

async fn handle_pools_spo_blockfrost(
    context: Arc<Context<Message>>,
    pool_operator: Vec<u8>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_history_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_metadata_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_relays_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_delegators_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_updates_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_pool_votes_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}
