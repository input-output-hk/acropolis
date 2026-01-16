//! MCP Resources
//!
//! Wraps existing Blockfrost handlers to expose them as MCP resources.
//! Uses the shared routes module from rest_blockfrost for consistency.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use config::Config;
use serde_json::Value;

use acropolis_common::messages::Message;
use acropolis_common::rest_error::RESTError;
use caryatid_sdk::Context;

use acropolis_module_rest_blockfrost::handlers_config::HandlersConfig;
use acropolis_module_rest_blockfrost::routes::{self, RouteDefinition, ROUTES};

/// Get all available MCP resources from the routes registry
pub fn get_all_resources() -> &'static [RouteDefinition] {
    ROUTES
}

/// Generic resource handler that dispatches to the appropriate Blockfrost handler
/// based on the MCP URI
pub async fn handle_resource(
    context: Arc<Context<Message>>,
    config: Arc<Config>,
    uri: &str,
) -> Result<Value> {
    // Match the URI against our routes
    let (route, params) =
        routes::match_uri(uri).ok_or_else(|| anyhow::anyhow!("Unknown resource URI: {uri}"))?;

    let handlers_config = Arc::new(HandlersConfig::from(config.clone()));

    // Dispatch to the appropriate handler based on the route
    let rest_response = dispatch_handler(context, params, HashMap::new(), handlers_config, route)
        .await
        .map_err(|e| anyhow::anyhow!("Handler error: {e}"))?;

    let json_value: Value = serde_json::from_str(&rest_response.body)
        .map_err(|e| anyhow::anyhow!("Failed to parse JSON response: {e}"))?;

    Ok(json_value)
}

/// Generic resource handler with query parameters
pub async fn handle_resource_with_query(
    context: Arc<Context<Message>>,
    config: Arc<Config>,
    uri: &str,
    query_params: HashMap<String, String>,
) -> Result<Value> {
    let (route, params) =
        routes::match_uri(uri).ok_or_else(|| anyhow::anyhow!("Unknown resource URI: {uri}"))?;

    let handlers_config = Arc::new(HandlersConfig::from(config.clone()));

    let rest_response = dispatch_handler(context, params, query_params, handlers_config, route)
        .await
        .map_err(|e| anyhow::anyhow!("Handler error: {e}"))?;

    let json_value: Value = serde_json::from_str(&rest_response.body)
        .map_err(|e| anyhow::anyhow!("Failed to parse JSON response: {e}"))?;

    Ok(json_value)
}

/// Dispatch to the correct handler based on the route definition
async fn dispatch_handler(
    context: Arc<Context<Message>>,
    params: Vec<String>,
    query_params: HashMap<String, String>,
    handlers_config: Arc<HandlersConfig>,
    route: &RouteDefinition,
) -> Result<acropolis_common::messages::RESTResponse, RESTError> {
    use acropolis_module_rest_blockfrost::handlers::{
        accounts::*, addresses::*, assets::*, blocks::*, epochs::*, governance::*, pools::*,
        transactions::*,
    };

    // Match on handler name and call the appropriate function
    match route.handler_name {
        // Accounts
        "handle_single_account_blockfrost" => {
            handle_single_account_blockfrost(context, params, handlers_config).await
        }
        "handle_account_registrations_blockfrost" => {
            handle_account_registrations_blockfrost(context, params, handlers_config).await
        }
        "handle_account_delegations_blockfrost" => {
            handle_account_delegations_blockfrost(context, params, handlers_config).await
        }
        "handle_account_mirs_blockfrost" => {
            handle_account_mirs_blockfrost(context, params, handlers_config).await
        }
        "handle_account_withdrawals_blockfrost" => {
            handle_account_withdrawals_blockfrost(context, params, handlers_config).await
        }
        "handle_account_rewards_blockfrost" => {
            handle_account_rewards_blockfrost(context, params, handlers_config).await
        }
        "handle_account_addresses_blockfrost" => {
            handle_account_addresses_blockfrost(context, params, handlers_config).await
        }
        "handle_account_assets_blockfrost" => {
            handle_account_assets_blockfrost(context, params, handlers_config).await
        }
        "handle_account_totals_blockfrost" => {
            handle_account_totals_blockfrost(context, params, handlers_config).await
        }
        "handle_account_utxos_blockfrost" => {
            handle_account_utxos_blockfrost(context, params, handlers_config).await
        }

        // Blocks
        "handle_blocks_latest_hash_number_blockfrost" => {
            handle_blocks_latest_hash_number_blockfrost(context, params, handlers_config).await
        }
        "handle_blocks_latest_hash_number_transactions_blockfrost" => {
            handle_blocks_latest_hash_number_transactions_blockfrost(
                context,
                params,
                query_params,
                handlers_config,
            )
            .await
        }
        "handle_blocks_latest_hash_number_transactions_cbor_blockfrost" => {
            handle_blocks_latest_hash_number_transactions_cbor_blockfrost(
                context,
                params,
                query_params,
                handlers_config,
            )
            .await
        }
        "handle_blocks_hash_number_next_blockfrost" => {
            handle_blocks_hash_number_next_blockfrost(
                context,
                params,
                query_params,
                handlers_config,
            )
            .await
        }
        "handle_blocks_hash_number_previous_blockfrost" => {
            handle_blocks_hash_number_previous_blockfrost(
                context,
                params,
                query_params,
                handlers_config,
            )
            .await
        }
        "handle_blocks_slot_blockfrost" => {
            handle_blocks_slot_blockfrost(context, params, handlers_config).await
        }
        "handle_blocks_epoch_slot_blockfrost" => {
            handle_blocks_epoch_slot_blockfrost(context, params, handlers_config).await
        }
        "handle_blocks_hash_number_addresses_blockfrost" => {
            handle_blocks_hash_number_addresses_blockfrost(
                context,
                params,
                query_params,
                handlers_config,
            )
            .await
        }

        // Governance - DReps
        "handle_dreps_list_blockfrost" => {
            handle_dreps_list_blockfrost(context, params, handlers_config).await
        }
        "handle_single_drep_blockfrost" => {
            handle_single_drep_blockfrost(context, params, handlers_config).await
        }
        "handle_drep_delegators_blockfrost" => {
            handle_drep_delegators_blockfrost(context, params, handlers_config).await
        }
        "handle_drep_metadata_blockfrost" => {
            handle_drep_metadata_blockfrost(context, params, handlers_config).await
        }
        "handle_drep_updates_blockfrost" => {
            handle_drep_updates_blockfrost(context, params, handlers_config).await
        }
        "handle_drep_votes_blockfrost" => {
            handle_drep_votes_blockfrost(context, params, handlers_config).await
        }

        // Governance - Proposals
        "handle_proposals_list_blockfrost" => {
            handle_proposals_list_blockfrost(context, params, handlers_config).await
        }
        "handle_single_proposal_blockfrost" => {
            handle_single_proposal_blockfrost(context, params, handlers_config).await
        }
        "handle_proposal_parameters_blockfrost" => {
            handle_proposal_parameters_blockfrost(context, params, handlers_config).await
        }
        "handle_proposal_withdrawals_blockfrost" => {
            handle_proposal_withdrawals_blockfrost(context, params, handlers_config).await
        }
        "handle_proposal_votes_blockfrost" => {
            handle_proposal_votes_blockfrost(context, params, handlers_config).await
        }
        "handle_proposal_metadata_blockfrost" => {
            handle_proposal_metadata_blockfrost(context, params, handlers_config).await
        }

        // Pools
        "handle_pools_list_blockfrost" => {
            handle_pools_list_blockfrost(context, params, handlers_config).await
        }
        "handle_pools_extended_retired_retiring_single_blockfrost" => {
            handle_pools_extended_retired_retiring_single_blockfrost(
                context,
                params,
                handlers_config,
            )
            .await
        }
        "handle_pool_history_blockfrost" => {
            handle_pool_history_blockfrost(context, params, handlers_config).await
        }
        "handle_pool_metadata_blockfrost" => {
            handle_pool_metadata_blockfrost(context, params, handlers_config).await
        }
        "handle_pool_relays_blockfrost" => {
            handle_pool_relays_blockfrost(context, params, handlers_config).await
        }
        "handle_pool_delegators_blockfrost" => {
            handle_pool_delegators_blockfrost(context, params, handlers_config).await
        }
        "handle_pool_blocks_blockfrost" => {
            handle_pool_blocks_blockfrost(context, params, handlers_config).await
        }
        "handle_pool_updates_blockfrost" => {
            handle_pool_updates_blockfrost(context, params, handlers_config).await
        }
        "handle_pool_votes_blockfrost" => {
            handle_pool_votes_blockfrost(context, params, handlers_config).await
        }

        // Epochs
        "handle_epoch_info_blockfrost" => {
            handle_epoch_info_blockfrost(context, params, handlers_config).await
        }
        "handle_epoch_params_blockfrost" => {
            handle_epoch_params_blockfrost(context, params, handlers_config).await
        }
        "handle_epoch_next_blockfrost" => {
            handle_epoch_next_blockfrost(context, params, handlers_config).await
        }
        "handle_epoch_previous_blockfrost" => {
            handle_epoch_previous_blockfrost(context, params, handlers_config).await
        }
        "handle_epoch_total_stakes_blockfrost" => {
            handle_epoch_total_stakes_blockfrost(context, params, handlers_config).await
        }
        "handle_epoch_pool_stakes_blockfrost" => {
            handle_epoch_pool_stakes_blockfrost(context, params, handlers_config).await
        }
        "handle_epoch_total_blocks_blockfrost" => {
            handle_epoch_total_blocks_blockfrost(context, params, handlers_config).await
        }
        "handle_epoch_pool_blocks_blockfrost" => {
            handle_epoch_pool_blocks_blockfrost(context, params, handlers_config).await
        }

        // Assets
        "handle_assets_list_blockfrost" => {
            handle_assets_list_blockfrost(context, params, handlers_config).await
        }
        "handle_asset_single_blockfrost" => {
            handle_asset_single_blockfrost(context, params, handlers_config).await
        }
        "handle_asset_history_blockfrost" => {
            handle_asset_history_blockfrost(context, params, handlers_config).await
        }
        "handle_asset_transactions_blockfrost" => {
            handle_asset_transactions_blockfrost(context, params, handlers_config).await
        }
        "handle_asset_addresses_blockfrost" => {
            handle_asset_addresses_blockfrost(context, params, handlers_config).await
        }
        "handle_policy_assets_blockfrost" => {
            handle_policy_assets_blockfrost(context, params, handlers_config).await
        }

        // Addresses
        "handle_address_single_blockfrost" => {
            handle_address_single_blockfrost(context, params, handlers_config).await
        }
        "handle_address_extended_blockfrost" => {
            handle_address_extended_blockfrost(context, params, handlers_config).await
        }
        "handle_address_totals_blockfrost" => {
            handle_address_totals_blockfrost(context, params, handlers_config).await
        }
        "handle_address_utxos_blockfrost" => {
            handle_address_utxos_blockfrost(context, params, handlers_config).await
        }
        "handle_address_asset_utxos_blockfrost" => {
            handle_address_asset_utxos_blockfrost(context, params, handlers_config).await
        }
        "handle_address_transactions_blockfrost" => {
            handle_address_transactions_blockfrost(context, params, handlers_config).await
        }

        // Transactions
        "handle_transactions_blockfrost" => {
            handle_transactions_blockfrost(context, params, handlers_config).await
        }

        _ => Err(RESTError::not_found(&format!(
            "Handler not implemented: {}",
            route.handler_name
        ))),
    }
}
