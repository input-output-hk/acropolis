use crate::{
    handlers_config::HandlersConfig,
    types::{
        EpochActivityRest, ProtocolParamsRest, SPDDByEpochAndPoolItemRest, SPDDByEpochItemRest,
    },
};
use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse},
        epochs::{EpochsStateQuery, EpochsStateQueryResponse},
        parameters::{ParametersStateQuery, ParametersStateQueryResponse},
        pools::{PoolsStateQuery, PoolsStateQueryResponse},
        spdd::{SPDDStateQuery, SPDDStateQueryResponse},
        utils::query_state,
    },
    serialization::Bech32WithHrp,
    NetworkId, StakeAddress, StakeCredential,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;
use std::sync::Arc;

pub async fn handle_epoch_info_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    if params.len() != 1 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected one parameter: 'latest' or an epoch number",
        ));
    }
    let param = &params[0];
    let query;

    // query to get latest epoch or epoch info
    if param == "latest" {
        query = EpochsStateQuery::GetLatestEpoch;
    } else {
        let parsed = match param.parse::<u64>() {
            Ok(num) => num,
            Err(_) => {
                return Ok(RESTResponse::with_text(
                    400,
                    "Invalid epoch number parameter",
                ));
            }
        };
        query = EpochsStateQuery::GetEpochInfo {
            epoch_number: parsed,
        };
    }

    // Get the current epoch number from epochs-state
    let epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(query)));
    let epoch_info_response = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        epoch_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(response)) => Ok(response),
            _ => {
                return Err(anyhow!(
                    "Unexpected message type while retrieving latest epoch"
                ))
            }
        },
    )
    .await?;

    let ea_message = match epoch_info_response {
        EpochsStateQueryResponse::LatestEpoch(response) => Ok(response.epoch),
        EpochsStateQueryResponse::EpochInfo(response) => Ok(response.epoch),
        EpochsStateQueryResponse::NotFound => Err(anyhow!("Epoch not found")),
        EpochsStateQueryResponse::Error(e) => Err(anyhow!(
            "Internal server error while retrieving epoch info: {e}"
        )),
        _ => Err(anyhow!(
            "Unexpected message type while retrieving epoch info"
        )),
    }?;
    let epoch_number = ea_message.epoch;

    // For the latest epoch, query accounts-state for the stake pool delegation distribution (SPDD)
    // Otherwise, fall back to SPDD module to fetch historical epoch totals
    let total_active_stakes: u64 = if param == "latest" {
        let total_active_stakes_msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
            AccountsStateQuery::GetActiveStakes {},
        )));
        query_state(
            &context,
            &handlers_config.accounts_query_topic,
            total_active_stakes_msg,
            |message| match message {
                Message::StateQueryResponse(StateQueryResponse::Accounts(
                    AccountsStateQueryResponse::ActiveStakes(total_active_stake),
                )) => Ok(total_active_stake),
                _ => Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving the latest total active stakes",
                )),
            },
        )
        .await?
    } else {
        // Historical epoch: use SPDD if available
        let total_active_stakes_msg = Arc::new(Message::StateQuery(StateQuery::SPDD(
            SPDDStateQuery::GetEpochTotalActiveStakes {
                epoch: epoch_number,
            },
        )));
        query_state(
            &context,
            &handlers_config.spdd_query_topic,
            total_active_stakes_msg,
            |message| match message {
                Message::StateQueryResponse(StateQueryResponse::SPDD(
                    SPDDStateQueryResponse::EpochTotalActiveStakes(total_active_stakes),
                )) => Ok(total_active_stakes),
                _ => Err(anyhow::anyhow!(
                    "Unexpected message type while retrieving total active stakes for epoch: {epoch_number}",
                )),
            },
        )
        .await?
    };

    let mut response = EpochActivityRest::from(ea_message);

    if total_active_stakes == 0 {
        response.active_stake = None;
    } else {
        response.active_stake = Some(total_active_stakes);
    }

    let json = match serde_json::to_string_pretty(&response) {
        Ok(j) => j,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Internal server error while retrieving latest epoch: {e}"),
            ));
        }
    };
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_epoch_params_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    if params.len() != 1 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected one parameter: 'latest' or an epoch number",
        ));
    }
    let param = &params[0];

    let query;
    let mut epoch_number: Option<u64> = None;

    // Get current epoch number from epochs-state
    let latest_epoch_info_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        latest_epoch_info_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch.epoch),
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

    if param == "latest" {
        query = ParametersStateQuery::GetLatestEpochParameters;
    } else {
        let parsed = match param.parse::<u64>() {
            Ok(num) => num,
            Err(_) => {
                return Ok(RESTResponse::with_text(
                    400,
                    "Invalid epoch number parameter",
                ));
            }
        };
        query = ParametersStateQuery::GetEpochParameters {
            epoch_number: parsed,
        };
        epoch_number = Some(parsed);
    }

    let parameters_msg = Arc::new(Message::StateQuery(StateQuery::Parameters(query)));
    let parameters_response = query_state(
        &context,
        &handlers_config.parameters_query_topic,
        parameters_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Parameters(resp)) => Ok(resp),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving parameters"
            )),
        },
    )
    .await?;

    match parameters_response {
        ParametersStateQueryResponse::LatestEpochParameters(params) => {
            let rest = ProtocolParamsRest::from((latest_epoch, params));
            match serde_json::to_string_pretty(&rest) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to serialize parameters: {e}"),
                )),
            }
        }
        ParametersStateQueryResponse::EpochParameters(params) => {
            let epoch = epoch_number.expect("epoch_number must exist for EpochParameters");

            if epoch > latest_epoch {
                return Ok(RESTResponse::with_text(
                    404,
                    "Protocol parameters not found for requested epoch",
                ));
            }
            let rest = ProtocolParamsRest::from((epoch, params));
            match serde_json::to_string_pretty(&rest) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Failed to serialize parameters: {e}"),
                )),
            }
        }
        ParametersStateQueryResponse::NotFound => Ok(RESTResponse::with_text(
            404,
            "Protocol parameters not found for requested epoch",
        )),
        ParametersStateQueryResponse::Error(msg) => Ok(RESTResponse::with_text(400, &msg)),
        _ => Ok(RESTResponse::with_text(
            500,
            "Unexpected message type while retrieving parameters",
        )),
    }
}

pub async fn handle_epoch_next_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    if params.len() != 1 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected one parameter: an epoch number",
        ));
    }
    let param = &params[0];

    let parsed = match param.parse::<u64>() {
        Ok(num) => num,
        Err(_) => {
            return Ok(RESTResponse::with_text(
                400,
                "Invalid epoch number parameter",
            ));
        }
    };

    let next_epochs_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetNextEpochs {
            epoch_number: parsed,
        },
    )));
    let next_epochs = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        next_epochs_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::NextEpochs(response),
            )) => Ok(response
                .epochs
                .into_iter()
                .map(|epoch| EpochActivityRest::from(epoch))
                .collect::<Vec<_>>()),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving next epochs: {e}"
                ));
            }
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!("Epoch not found")),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving next epochs"
            )),
        },
    )
    .await?;

    let json = match serde_json::to_string_pretty(&next_epochs) {
        Ok(j) => j,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Failed to serialize epoch info: {e}"),
            ));
        }
    };
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_epoch_previous_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    if params.len() != 1 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected one parameter: an epoch number",
        ));
    }
    let param = &params[0];

    let parsed = match param.parse::<u64>() {
        Ok(num) => num,
        Err(_) => {
            return Ok(RESTResponse::with_text(
                400,
                "Invalid epoch number parameter",
            ));
        }
    };

    let previous_epochs_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetPreviousEpochs {
            epoch_number: parsed,
        },
    )));
    let previous_epochs = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        previous_epochs_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::PreviousEpochs(response),
            )) => Ok(response
                .epochs
                .into_iter()
                .map(|epoch| EpochActivityRest::from(epoch))
                .collect::<Vec<_>>()),
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::Error(e),
            )) => {
                return Err(anyhow::anyhow!(
                    "Internal server error while retrieving previous epochs: {e}"
                ));
            }
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::NotFound,
            )) => Err(anyhow::anyhow!("Epoch not found")),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving previous epochs"
            )),
        },
    )
    .await?;

    let json = match serde_json::to_string_pretty(&previous_epochs) {
        Ok(j) => j,
        Err(e) => {
            return Ok(RESTResponse::with_text(
                500,
                &format!("Failed to serialize epoch info: {e}"),
            ));
        }
    };
    Ok(RESTResponse::with_json(200, &json))
}

pub async fn handle_epoch_total_stakes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    if params.len() != 1 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected one parameter: an epoch number",
        ));
    }
    let param = &params[0];

    let epoch_number = match param.parse::<u64>() {
        Ok(num) => num,
        Err(_) => {
            return Ok(RESTResponse::with_text(
                400,
                "Invalid epoch number parameter",
            ));
        }
    };

    // Query latest epoch from epochs-state
    let latest_epoch_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        latest_epoch_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch.epoch),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving latest epoch"
            )),
        },
    )
    .await?;

    if epoch_number > latest_epoch {
        return Ok(RESTResponse::with_text(404, "Epoch not found"));
    }

    // Query current network from parameters-state
    let current_network_msg = Arc::new(Message::StateQuery(StateQuery::Parameters(
        ParametersStateQuery::GetNetworkName,
    )));
    let current_network = query_state(
        &context,
        &handlers_config.parameters_query_topic,
        current_network_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Parameters(
                ParametersStateQueryResponse::NetworkName(network),
            )) => Ok(network),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving current network"
            )),
        },
    )
    .await?;

    let network = match current_network.as_str() {
        "mainnet" => NetworkId::Mainnet,
        "testnet" => NetworkId::Testnet,
        unknown => {
            return Ok(RESTResponse::with_text(
                500,
                format!("Internal server error while retrieving current network: {unknown}")
                    .as_str(),
            ))
        }
    };

    // Query SPDD by epoch from accounts-state
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetSPDDByEpoch {
            epoch: epoch_number,
        },
    )));
    let spdd = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::SPDDByEpoch(res),
            )) => Ok(res),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving SPDD by epoch: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving SPDD by epoch"
            )),
        },
    )
    .await?;
    let spdd_response = spdd
        .into_iter()
        .map(|(pool_id, stake_key_hash, amount)| {
            let stake_address = StakeAddress {
                network: network.clone(),
                credential: StakeCredential::AddrKeyHash(stake_key_hash),
            }
            .to_string()
            .map_err(|e| anyhow::anyhow!("Failed to convert stake address to string: {e}"))?;
            Ok(SPDDByEpochItemRest {
                pool_id,
                stake_address,
                amount,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    match serde_json::to_string_pretty(&spdd_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Failed to serialize SPDD by epoch: {e}"),
        )),
    }
}

pub async fn handle_epoch_pool_stakes_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    if params.len() != 2 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected two parameters: an epoch number and a pool ID",
        ));
    }
    let param = &params[0];
    let pool_id = &params[1];

    let epoch_number = match param.parse::<u64>() {
        Ok(num) => num,
        Err(_) => {
            return Ok(RESTResponse::with_text(
                400,
                "Invalid epoch number parameter",
            ));
        }
    };

    let Ok(pool_id) = Vec::<u8>::from_bech32_with_hrp(pool_id, "pool") else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 stake pool ID: {pool_id}"),
        ));
    };

    // Query latest epoch from epochs-state
    let latest_epoch_msg = Arc::new(Message::StateQuery(StateQuery::Epochs(
        EpochsStateQuery::GetLatestEpoch,
    )));
    let latest_epoch = query_state(
        &context,
        &handlers_config.epochs_query_topic,
        latest_epoch_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Epochs(
                EpochsStateQueryResponse::LatestEpoch(res),
            )) => Ok(res.epoch.epoch),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving latest epoch"
            )),
        },
    )
    .await?;

    if epoch_number > latest_epoch {
        return Ok(RESTResponse::with_text(404, "Epoch not found"));
    }

    // Query current network from parameters-state
    let current_network_msg = Arc::new(Message::StateQuery(StateQuery::Parameters(
        ParametersStateQuery::GetNetworkName,
    )));
    let current_network = query_state(
        &context,
        &handlers_config.parameters_query_topic,
        current_network_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Parameters(
                ParametersStateQueryResponse::NetworkName(network),
            )) => Ok(network),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving current network"
            )),
        },
    )
    .await?;

    let network = match current_network.as_str() {
        "mainnet" => NetworkId::Mainnet,
        "testnet" => NetworkId::Testnet,
        unknown => {
            return Ok(RESTResponse::with_text(
                500,
                format!("Internal server error while retrieving current network: {unknown}")
                    .as_str(),
            ))
        }
    };

    // Query SPDD by epoch and pool from accounts-state
    let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
        AccountsStateQuery::GetSPDDByEpochAndPool {
            epoch: epoch_number,
            pool_id,
        },
    )));
    let spdd = query_state(
        &context,
        &handlers_config.accounts_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::SPDDByEpochAndPool(res),
            )) => Ok(res),
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving SPDD by epoch and pool: {e}"
            )),
            _ => Err(anyhow::anyhow!(
                "Unexpected message type while retrieving SPDD by epoch and pool"
            )),
        },
    )
    .await?;
    let spdd_response = spdd
        .into_iter()
        .map(|(key_hash, amount)| {
            let stake_address = StakeAddress {
                network: network.clone(),
                credential: StakeCredential::AddrKeyHash(key_hash),
            }
            .to_string()
            .map_err(|e| anyhow::anyhow!("Failed to convert stake address to string: {e}"))?;

            Ok(SPDDByEpochAndPoolItemRest {
                stake_address,
                amount,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    match serde_json::to_string_pretty(&spdd_response) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Failed to serialize SPDD by epoch and pool: {e}"),
        )),
    }
}

pub async fn handle_epoch_total_blocks_blockfrost(
    _context: Arc<Context<Message>>,
    _params: Vec<String>,
    _handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    Ok(RESTResponse::with_text(501, "Not implemented"))
}

pub async fn handle_epoch_pool_blocks_blockfrost(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    if params.len() != 2 {
        return Ok(RESTResponse::with_text(
            400,
            "Expected two parameters: an epoch number and a pool ID",
        ));
    }
    let epoch_number_param = &params[0];
    let pool_id_param = &params[1];

    let epoch_number = match epoch_number_param.parse::<u64>() {
        Ok(num) => num,
        Err(_) => {
            return Ok(RESTResponse::with_text(
                400,
                "Invalid epoch number parameter",
            ));
        }
    };

    let Ok(spo) = Vec::<u8>::from_bech32_with_hrp(pool_id_param, "pool") else {
        return Ok(RESTResponse::with_text(
            400,
            &format!("Invalid Bech32 stake pool ID: {pool_id_param}"),
        ));
    };

    // query Pool's Blocks by epoch from spo-state
    let msg = Arc::new(Message::StateQuery(StateQuery::Pools(
        PoolsStateQuery::GetBlocksByPoolAndEpoch {
            pool_id: spo.clone(),
            epoch: epoch_number,
        },
    )));

    let blocks = query_state(
        &context,
        &handlers_config.pools_query_topic,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::BlocksByPoolAndEpoch(blocks),
            )) => Ok(blocks),
            Message::StateQueryResponse(StateQueryResponse::Pools(
                PoolsStateQueryResponse::Error(e),
            )) => Err(anyhow::anyhow!(
                "Internal server error while retrieving pool block hashes by epoch: {e}"
            )),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        },
    )
    .await?;

    // NOTE:
    // Need to query chain_store
    // to get block_hash for each block height

    match serde_json::to_string_pretty(&blocks) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while retrieving pool block hashes by epoch: {e}"),
        )),
    }
}
