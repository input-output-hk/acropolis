//! REST handlers for Acropolis Blockfrost /pools endpoints
use acropolis_common::{messages::{Message, RESTResponse, StateQuery, StateQueryResponse}, queries::{
    accounts::{AccountsStateQuery, AccountsStateQueryResponse},
    epochs::{EpochsStateQuery, EpochsStateQueryResponse},
    pools::{PoolsStateQuery, PoolsStateQueryResponse},
    utils::query_state,
}, rest_helper::ToCheckedF64, serialization::Bech32WithHrp, PoolId, PoolRetirement, PoolUpdateAction, StakeCredential, TxHash};
use anyhow::Result;
use caryatid_sdk::Context;
use rust_decimal::Decimal;
use std::{sync::Arc, time::Duration};
use tokio::join;
use tracing::warn;
use acropolis_common::serialization::Bech32Conversion;
use crate::{
    handlers_config::HandlersConfig,
    types::{PoolDelegatorRest, PoolInfoRest, PoolRelayRest, PoolUpdateEventRest, PoolVoteRest},
};
use crate::{
    types::{PoolEpochStateRest, PoolExtendedRest, PoolMetadataRest, PoolRetirementRest},
    utils::{fetch_pool_metadata_as_bytes, verify_pool_metadata_hash, PoolMetadataJson},
};

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
            PoolsStateQueryResponse::PoolsList(pool_operators),
        )) => pool_operators,

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
        .map(|operator| operator.to_bech32())
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
        _ => match PoolId::from_bech32(param) {
            Ok(pool_id) => {
                return handle_pools_spo_blockfrost(
                    context.clone(),
                    pool_id,
                    handlers_config.clone(),
                )
                .await
            }
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
    let pools_list_with_info_f = query_state(
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
    );

    // Get Latest Epoch from epochs-state
    let latest_epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch_info_f = query_state(
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
    );

    // Get optimal_pool_sizing from accounts_state
    let optimal_pool_sizing_msg: Arc<Message> = Arc::new(Message::StateQuery(
        StateQuery::Accounts(AccountsStateQuery::GetOptimalPoolSizing),
    ));
    let optimal_pool_sizing_f = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        optimal_pool_sizing_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::OptimalPoolSizing(res),
            )) => Ok(res),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    let (pools_list_with_info, latest_epoch_info, optimal_pool_sizing) = join!(
        pools_list_with_info_f,
        latest_epoch_info_f,
        optimal_pool_sizing_f
    );
    let pools_list_with_info = pools_list_with_info?;
    let latest_epoch_info = latest_epoch_info?;
    let latest_epoch = latest_epoch_info.epoch;
    let optimal_pool_sizing = optimal_pool_sizing?;

    // if pools are empty, return an empty list
    if pools_list_with_info.is_empty() {
        return Ok(RESTResponse::with_json(200, "[]"));
    }

    // check optimal_pool_sizing is Some
    let Some(optimal_pool_sizing) = optimal_pool_sizing else {
        // if it is before Shelly Era
        return Ok(RESTResponse::with_json(200, "[]"));
    };

    // Populate pools_operators
    let pools_operators = pools_list_with_info
        .iter()
        .map(|(pool_operator, _)| pool_operator.clone())
        .collect::<Vec<_>>();

    // Get an active stake for each pool from spo-state
    let pools_active_stakes_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsActiveStakes {
            pools_operators: pools_operators.clone(),
            epoch: latest_epoch,
        },
    )));
    let pools_active_stakes_f = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pools_active_stakes_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolsActiveStakes(active_stakes),
            )) => Ok(Some(active_stakes)),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(_e),
            )) => {
                // if epoch_history is not enabled
                Ok(None)
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving pools active stakes"
                ))
            }
        },
    );

    // Get live stake for each pool from accounts-state
    let pools_live_stakes_msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetPoolsLiveStakes {
            pools_operators: pools_operators.clone(),
        },
    )));
    let pools_live_stakes_f = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        pools_live_stakes_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::PoolsLiveStakes(pools_live_stakes),
            )) => Ok(pools_live_stakes),

            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving pools live stakes: {e}"
                ));
            }

            _ => return Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    // Get total blocks minted for each pool from spo-state
    let total_blocks_minted_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolsTotalBlocksMinted {
            pools_operators: pools_operators.clone(),
        },
    )));
    let total_blocks_minted_f = query_state(
        &context,
        &handlers_config.pools_query_topic,
        total_blocks_minted_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolsTotalBlocksMinted(total_blocks_minted),
            )) => Ok(total_blocks_minted),
            _ => return Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    let (pools_active_stakes, pools_live_stakes, total_blocks_minted) = join!(
        pools_active_stakes_f,
        pools_live_stakes_f,
        total_blocks_minted_f
    );
    let pools_active_stakes = pools_active_stakes?;
    let pools_live_stakes = pools_live_stakes?;
    let total_blocks_minted = total_blocks_minted?;

    let pools_extened_rest_results: Result<Vec<PoolExtendedRest>, anyhow::Error> =
        pools_list_with_info
            .iter()
            .enumerate()
            .map(|(i, (pool_operator, pool_registration))| {
                Ok(PoolExtendedRest {
                    pool_id: pool_operator.to_bech32()?,
                    hex: pool_operator.to_vec(),
                    active_stake: pools_active_stakes
                        .as_ref()
                        .map(|active_stakes| active_stakes[i]),
                    live_stake: pools_live_stakes[i],
                    blocks_minted: total_blocks_minted[i],
                    live_saturation: Decimal::from(pools_live_stakes[i])
                        * Decimal::from(optimal_pool_sizing.nopt)
                        / Decimal::from(optimal_pool_sizing.total_supply),
                    declared_pledge: pool_registration.pledge,
                    margin_cost: pool_registration.margin.to_f32(),
                    fixed_cost: pool_registration.cost,
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
            )) => Ok(retired_pools),
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
            let pool_id = operator.to_bech32().ok()?;
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
            )) => Ok(retiring_pools),
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
            let pool_id = operator.to_bech32().ok()?;
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
    context: Arc<Context<Message>>,
    pool_operator: PoolId,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    // Get PoolRegistration from spo state
    let pool_info_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolInfo {
            pool_id: pool_operator.clone(),
        },
    )));

    let pool_info_f = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pool_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolInfo(pool_info),
            )) => Ok(pool_info),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!("Pool Not found")),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving pool info: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    // Get Latest Epoch from epochs-state
    let latest_epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch_info_f = query_state(
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
    );

    // query live stakes from accounts_state
    let live_stakes_info_msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetPoolLiveStake {
            pool_operator: pool_operator.clone(),
        },
    )));
    let live_stakes_info_f = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        live_stakes_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::PoolLiveStake(res),
            )) => Ok(res),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    // Get optimal_pool_sizing from accounts_state
    let optimal_pool_sizing_msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetOptimalPoolSizing,
    )));
    let optimal_pool_sizing_f = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        optimal_pool_sizing_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::OptimalPoolSizing(res),
            )) => Ok(res),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    // Query pool update events from spo_state
    let pool_updates_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolUpdates {
            pool_id: pool_operator.clone(),
        },
    )));
    let pool_updates_f = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pool_updates_msg,
        |message: Message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolUpdates(pool_updates),
            )) => Ok(Some(pool_updates)),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!("Pool Not found")),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(_e),
            )) => Ok(None),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    // Query total_blocks_minted from spo_state
    let total_blocks_minted_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolTotalBlocksMinted {
            pool_id: pool_operator.clone(),
        },
    )));
    let total_blocks_minted_f = query_state(
        &context,
        &handlers_config.pools_query_topic,
        total_blocks_minted_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolTotalBlocksMinted(total_blocks_minted),
            )) => Ok(total_blocks_minted),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    let (
        pool_info,
        latest_epoch_info,
        live_stakes_info,
        optimal_pool_sizing,
        pool_updates,
        total_blocks_minted,
    ) = join!(
        pool_info_f,
        latest_epoch_info_f,
        live_stakes_info_f,
        optimal_pool_sizing_f,
        pool_updates_f,
        total_blocks_minted_f,
    );
    let pool_info = pool_info?;
    let latest_epoch_info = latest_epoch_info?;
    let latest_epoch = latest_epoch_info.epoch;
    let live_stakes_info = live_stakes_info?;
    let total_blocks_minted = total_blocks_minted?;
    let Some(optimal_pool_sizing) = optimal_pool_sizing? else {
        // if it is before Shelly Era
        return Ok(RESTResponse::with_json(404, "Pool Not Found"));
    };
    let pool_updates = pool_updates?;
    let registrations: Option<Vec<TxHash>> = pool_updates.as_ref().map(|updates| {
        updates
            .iter()
            .filter_map(|update| {
                if update.action == PoolUpdateAction::Registered {
                    Some(update.tx_hash)
                } else {
                    None
                }
            })
            .collect()
    });
    let retirements: Option<Vec<TxHash>> = pool_updates.as_ref().map(|updates| {
        updates
            .iter()
            .filter_map(|update| {
                if update.action == PoolUpdateAction::Deregistered {
                    Some(update.tx_hash)
                } else {
                    None
                }
            })
            .collect()
    });

    // Query blocks_minted from epochs_state
    let epoch_blocks_minted_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpochBlocksMintedByPool {
            spo_id: pool_info.operator.clone(),
        },
    )));
    let epoch_blocks_minted_f = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        epoch_blocks_minted_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpochBlocksMintedByPool(blocks_minted),
            )) => Ok(blocks_minted),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    // query active stakes info from spo_state
    let active_stakes_info_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolActiveStakeInfo {
            pool_operator: pool_operator.clone(),
            epoch: latest_epoch,
        },
    )));
    let active_stakes_info_f = query_state(
        &context,
        &handlers_config.pools_query_topic,
        active_stakes_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolActiveStakeInfo(res),
            )) => Ok(Some(res)),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(_e),
            )) => Ok(None),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    // Get live_pledge
    // Query owner accounts balance sum from accounts_state
    let live_pledge_msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetAccountsUtxoValuesSum {
            stake_addresses: pool_info.pool_owners.clone(),
        },
    )));

    let live_pledge_f = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        live_pledge_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountsUtxoValuesSum(res),
            )) => Ok(res),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving live pledge: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    );

    let (epoch_blocks_minted, active_stakes_info, live_pledge) =
        join!(epoch_blocks_minted_f, active_stakes_info_f, live_pledge_f,);
    let epoch_blocks_minted = epoch_blocks_minted?;
    let active_stakes_info = active_stakes_info?;
    let live_pledge = live_pledge?;

    let pool_id = pool_info.operator.to_bech32()?;
    let reward_account = pool_info.reward_account.get_credential().to_stake_bech32();
    let Ok(reward_account) = reward_account else {
        return Ok(RESTResponse::with_text(404, "Invalid Reward Account"));
    };
    let pool_owners = pool_info
        .pool_owners
        .iter()
        .map(|owner| owner.get_credential().to_stake_bech32())
        .collect::<Result<Vec<String>, _>>();
    let Ok(pool_owners) = pool_owners else {
        return Ok(RESTResponse::with_text(404, "Invalid Pool Owners"));
    };
    let pool_info_rest: PoolInfoRest = PoolInfoRest {
        pool_id,
        hex: pool_info.operator,
        vrf_key: pool_info.vrf_key_hash,
        blocks_minted: total_blocks_minted,
        blocks_epoch: epoch_blocks_minted,
        live_stake: live_stakes_info.live_stake,
        live_size: Decimal::from(live_stakes_info.live_stake)
            / Decimal::from(live_stakes_info.total_live_stakes),
        live_saturation: Decimal::from(live_stakes_info.live_stake)
            * Decimal::from(optimal_pool_sizing.nopt)
            / Decimal::from(optimal_pool_sizing.total_supply),
        live_delegators: live_stakes_info.live_delegators,
        active_stake: active_stakes_info.as_ref().map(|info| info.active_stake),
        active_size: active_stakes_info
            .as_ref()
            .map(|info| info.active_size.to_checked_f64("active_size").unwrap_or(0.0)),
        declared_pledge: pool_info.pledge,
        live_pledge: live_pledge,
        margin_cost: pool_info.margin.to_f32(),
        fixed_cost: pool_info.cost,
        reward_account,
        pool_owners,
        registration: registrations,
        retirement: retirements,
    };

    match serde_json::to_string(&pool_info_rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pool info: {e}"),
        )),
    }
}

pub async fn handle_pool_history_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(pool_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing pool ID parameter"));
    };

    let Ok(spo) = PoolId::from_bech32(pool_id) else {
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

    // Get pool history from spo-state
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
            )) => Ok(pool_history.into_iter().map(|state| state.into()).collect()),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(_e),
            )) => {
                // when pool epoch history is not enabled
                Err(anyhow::anyhow!("Pool Epoch History is not enabled."))
            }
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

    let Ok(spo) = PoolId::from_bech32(pool_id) else {
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

    let pool_metadata_bytes = fetch_pool_metadata_as_bytes(
        pool_metadata.url.clone(),
        Duration::from_secs(handlers_config.external_api_timeout),
    )
    .await?;

    // Verify hash of the fetched pool metadata, matches with the metadata hash provided by PoolRegistration
    if let Err(e) = verify_pool_metadata_hash(&pool_metadata_bytes, &pool_metadata.hash) {
        return Ok(RESTResponse::with_text(404, &e));
    }

    // Convert bytes into an understandable PoolMetadata structure
    let Ok(pool_metadata_json) = PoolMetadataJson::try_from(pool_metadata_bytes) else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Failed PoolMetadata Json conversion"),
        ));
    };

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
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(pool_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing pool ID parameter"));
    };

    let Ok(spo) = PoolId::from_bech32(pool_id) else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 stake pool ID: {pool_id}"),
        ));
    };

    let pool_relay_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolRelays {
            pool_id: spo.clone(),
        },
    )));

    let pool_relays = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pool_relay_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolRelays(pool_relays),
            )) => Ok(pool_relays),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!("Pool Relays Not found")),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving pool relays: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    let relays_in_rest = pool_relays.into_iter().map(|r| r.into()).collect::<Vec<PoolRelayRest>>();

    match serde_json::to_string(&relays_in_rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pool relays: {e}"),
        )),
    }
}

pub async fn handle_pool_delegators_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(pool_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing pool ID parameter"));
    };

    let Ok(spo) = PoolId::from_bech32(pool_id) else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 stake pool ID: {pool_id}"),
        ));
    };

    // Get Pool delegators from spo-state
    let pool_delegators_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolDelegators {
            pool_id: spo.clone(),
        },
    )));

    let pool_delegators = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pool_delegators_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolDelegators(pool_delegators),
            )) => Ok(Some(pool_delegators.delegators)),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!("Pool Delegators Not found")),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(_e),
            )) => {
                // store-stake-addresses is not enabled
                warn!("Fallback to query from accounts_state");
                Ok(None)
            }
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    // Get pool_delegators from accounts-state as fallback
    let pool_delegators = match pool_delegators {
        Some(delegators) => delegators,
        None => {
            // Query from Accounts state
            let pool_delegators_msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
                AccountsStateQuery::GetPoolDelegators {
                    pool_operator: spo.clone(),
                },
            )));
            let pool_delegators = query_state(
                &context,
                &handlers_config.accounts_query_topic,
                pool_delegators_msg,
                |message| match message {
                    Message::StateQueryResponse(StateQueryResponse::Accounts(
                        AccountsStateQueryResponse::PoolDelegators(pool_delegators),
                    )) => Ok(pool_delegators.delegators),
                    Message::StateQueryResponse(StateQueryResponse::Accounts(
                        AccountsStateQueryResponse::Error(e),
                    )) => Err(anyhow::anyhow!(
                        "Error while retrieving pool delegators from accounts_state: {e}"
                    )),
                    _ => Err(anyhow::anyhow!("Unexpected message type")),
                },
            )
            .await?;
            pool_delegators
        }
    };

    let mut delegators_rest = Vec::<PoolDelegatorRest>::new();
    for (d, l) in pool_delegators {
        let bech32 = StakeCredential::AddrKeyHash(d.clone())
            .to_stake_bech32()
            .map_err(|e| anyhow::anyhow!("Invalid stake address in pool delegators: {e}"))?;
        delegators_rest.push(PoolDelegatorRest {
            address: bech32,
            live_stake: l.to_string(),
        });
    }

    match serde_json::to_string(&delegators_rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pool delegators: {e}"),
        )),
    }
}

pub async fn handle_pool_blocks_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(pool_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing pool ID parameter"));
    };

    let Ok(spo) = PoolId::from_bech32(pool_id) else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 stake pool ID: {pool_id}"),
        ));
    };

    // Get blocks by pool_id from spo_state
    let pool_blocks_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetBlocksByPool {
            pool_id: spo.clone(),
        },
    )));

    let pool_blocks = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pool_blocks_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::BlocksByPool(pool_blocks),
            )) => Ok(pool_blocks),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(_),
            )) => Err(anyhow::anyhow!("Blocks are not enabled")),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    // NOTE:
    // Need to query chain_store
    // to get block_hash for each block height

    match serde_json::to_string_pretty(&pool_blocks) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pool blocks: {e}"),
        )),
    }
}

pub async fn handle_pool_updates_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(pool_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing pool ID parameter"));
    };

    let Ok(spo) = PoolId::from_bech32(pool_id) else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 stake pool ID: {pool_id}"),
        ));
    };

    // query from spo_state
    let pool_updates_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolUpdates {
            pool_id: spo.clone(),
        },
    )));
    let pool_updates = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pool_updates_msg,
        |message: Message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolUpdates(pool_updates),
            )) => Ok(pool_updates),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!("Pool not found")),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving pool updates: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;
    let pool_updates_rest = pool_updates
        .into_iter()
        .map(|u| PoolUpdateEventRest {
            tx_hash: u.tx_hash,
            cert_index: u.cert_index,
            action: u.action,
        })
        .collect::<Vec<_>>();

    match serde_json::to_string(&pool_updates_rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pool updates: {e}"),
        )),
    }
}

pub async fn handle_pool_votes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let Some(pool_id) = params.get(0) else {
        return Ok(RESTResponse::with_text(400, "Missing pool ID parameter"));
    };

    let Ok(spo) = PoolId::from_bech32(pool_id) else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 stake pool ID: {pool_id}"),
        ));
    };

    // query from spo_state
    let pool_votes_msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetPoolVotes {
            pool_id: spo.clone(),
        },
    )));
    let pool_votes = query_state(
        &context,
        &handlers_config.pools_query_topic,
        pool_votes_msg,
        |message: Message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::PoolVotes(pool_votes),
            )) => Ok(pool_votes),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving pool votes: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    let pool_votes_rest = pool_votes
        .into_iter()
        .map(|v| PoolVoteRest {
            tx_hash: v.tx_hash,
            vote_index: v.vote_index,
            vote: v.vote,
        })
        .collect::<Vec<_>>();

    match serde_json::to_string(&pool_votes_rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pool votes: {e}"),
        )),
    }
}
