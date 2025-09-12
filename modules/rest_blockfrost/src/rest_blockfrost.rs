//! Acropolis Blockfrost-Compatible REST Module

use std::{future::Future, sync::Arc};

use acropolis_common::{
    messages::{Message, RESTResponse},
    rest_helper::handle_rest_with_path_parameter,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use tracing::info;
mod cost_models;
mod handlers;
mod handlers_config;
mod types;
mod utils;
use handlers::{
    accounts::handle_single_account_blockfrost,
    assets::{
        handle_asset_addresses_blockfrost, handle_asset_history_blockfrost,
        handle_asset_transactions_blockfrost, handle_assets_list_blockfrost,
    },
    epochs::{
        handle_epoch_info_blockfrost, handle_epoch_next_blockfrost, handle_epoch_params_blockfrost,
        handle_epoch_pool_blocks_blockfrost, handle_epoch_pool_stakes_blockfrost,
        handle_epoch_previous_blockfrost, handle_epoch_total_blocks_blockfrost,
        handle_epoch_total_stakes_blockfrost,
    },
    governance::{
        handle_drep_delegators_blockfrost, handle_drep_metadata_blockfrost,
        handle_drep_updates_blockfrost, handle_drep_votes_blockfrost, handle_dreps_list_blockfrost,
        handle_proposal_metadata_blockfrost, handle_proposal_parameters_blockfrost,
        handle_proposal_votes_blockfrost, handle_proposal_withdrawals_blockfrost,
        handle_proposals_list_blockfrost, handle_single_drep_blockfrost,
        handle_single_proposal_blockfrost,
    },
    pools::{
        handle_pool_blocks_blockfrost, handle_pool_delegators_blockfrost,
        handle_pool_history_blockfrost, handle_pool_metadata_blockfrost,
        handle_pool_relays_blockfrost, handle_pool_updates_blockfrost,
        handle_pool_votes_blockfrost, handle_pools_extended_retired_retiring_single_blockfrost,
        handle_pools_list_blockfrost,
    },
};

use crate::{
    handlers::assets::{handle_asset_single_blockfrost, handle_policy_assets_blockfrost},
    handlers_config::HandlersConfig,
};

// Accounts topics
const DEFAULT_HANDLE_SINGLE_ACCOUNT_TOPIC: (&str, &str) =
    ("handle-topic-account-single", "rest.get.accounts.*");

// Governance topics
const DEFAULT_HANDLE_DREPS_LIST_TOPIC: (&str, &str) =
    ("handle-topic-dreps-list", "rest.get.governance.dreps");
const DEFAULT_HANDLE_SINGLE_DREP_TOPIC: (&str, &str) =
    ("handle-topic-dreps-single", "rest.get.governance.dreps.*");
const DEFAULT_HANDLE_DREP_DELEGATORS_TOPIC: (&str, &str) = (
    "handle-topic-dreps-delegators",
    "rest.get.governance.dreps.*.delegators",
);
const DEFAULT_HANDLE_DREP_METADATA_TOPIC: (&str, &str) = (
    "handle-topic-dreps-metadata",
    "rest.get.governance.dreps.*.metadata",
);
const DEFAULT_HANDLE_DREP_UPDATES_TOPIC: (&str, &str) = (
    "handle-topic-dreps-updates",
    "rest.get.governance.dreps.*.updates",
);
const DEFAULT_HANDLE_DREP_VOTES_TOPIC: (&str, &str) = (
    "handle-topic-dreps-votes",
    "rest.get.governance.dreps.*.votes",
);
const DEFAULT_HANDLE_PROPOSALS_LIST_TOPIC: (&str, &str) = (
    "handle-topic-proposals-list",
    "rest.get.governance.proposals",
);
const DEFAULT_HANDLE_SINGLE_PROPOSAL_TOPIC: (&str, &str) = (
    "handle-topic-proposals-single",
    "rest.get.governance.proposals.*.*",
);
const DEFAULT_HANDLE_PROPOSAL_PARAMETERS_TOPIC: (&str, &str) = (
    "handle-topic-proposals-parameters",
    "rest.get.governance.proposals.*.*.parameters",
);
const DEFAULT_HANDLE_PROPOSAL_WITHDRAWALS_TOPIC: (&str, &str) = (
    "handle-topic-proposals-withdrawals",
    "rest.get.governance.proposals.*.*.withdrawals",
);
const DEFAULT_HANDLE_PROPOSAL_VOTES_TOPIC: (&str, &str) = (
    "handle-topic-proposals-votes",
    "rest.get.governance.proposals.*.*.votes",
);
const DEFAULT_HANDLE_PROPOSAL_METADATA_TOPIC: (&str, &str) = (
    "handle-topic-proposals-metadata",
    "rest.get.governance.proposals.*.*.metadata",
);

// Pools topics
const DEFAULT_HANDLE_POOLS_LIST_TOPIC: (&str, &str) = ("handle-topic-pools-list", "rest.get.pools");
const DEFAULT_HANDLE_POOLS_EXTENDED_RETIRED_RETIRING_SINGLE_TOPIC: (&str, &str) = (
    "handle-topic-pools-extended-retired-retiring-single",
    "rest.get.pools.*",
);
const DEFAULT_HANDLE_POOL_HISTORY_TOPIC: (&str, &str) =
    ("handle-topic-pool-history", "rest.get.pools.*.history");
const DEFAULT_HANDLE_POOL_METADATA_TOPIC: (&str, &str) =
    ("handle-topic-pool-metadata", "rest.get.pools.*.metadata");
const DEFAULT_HANDLE_POOL_RELAYS_TOPIC: (&str, &str) =
    ("handle-topic-pool-relays", "rest.get.pools.*.relays");
const DEFAULT_HANDLE_POOL_DELEGATORS_TOPIC: (&str, &str) = (
    "handle-topic-pool-delegators",
    "rest.get.pools.*.delegators",
);
const DEFAULT_HANDLE_POOL_BLOCKS_TOPIC: (&str, &str) =
    ("handle-topic-pool-blocks", "rest.get.pools.*.blocks");
const DEFAULT_HANDLE_POOL_UPDATES_TOPIC: (&str, &str) =
    ("handle-topic-pool-updates", "rest.get.pools.*.updates");
const DEFAULT_HANDLE_POOL_VOTES_TOPIC: (&str, &str) =
    ("handle-topic-pool-votes", "rest.get.pools.*.votes");

// Epochs topics
const DEFAULT_HANDLE_EPOCH_INFO_TOPIC: (&str, &str) =
    ("handle-topic-epoch-info", "rest.get.epoch.*"); // Both latest and specific
const DEFAULT_HANDLE_EPOCH_PARAMS_TOPIC: (&str, &str) = (
    "handle-topic-epoch-parameters",
    "rest.get.epochs.*.parameters",
); // Both latest and specific
const DEFAULT_HANDLE_EPOCH_NEXT_TOPIC: (&str, &str) =
    ("handle-topic-epoch-next", "rest.get.epochs.*.next");
const DEFAULT_HANDLE_EPOCH_PREVIOUS_TOPIC: (&str, &str) =
    ("handle-topic-epoch-previous", "rest.get.epochs.*.previous");
const DEFAULT_HANDLE_EPOCH_TOTAL_STAKES_TOPIC: (&str, &str) = (
    "handle-topic-epoch-total-stakes",
    "rest.get.epochs.*.stakes",
);
const DEFAULT_HANDLE_EPOCH_POOL_STAKES_TOPIC: (&str, &str) = (
    "handle-topic-epoch-pool-stakes",
    "rest.get.epochs.*.stakes.*",
);
const DEFAULT_HANDLE_EPOCH_TOTAL_BLOCKS_TOPIC: (&str, &str) = (
    "handle-topic-epoch-total-blocks",
    "rest.get.epochs.*.blocks",
);
const DEFAULT_HANDLE_EPOCH_POOL_BLOCKS_TOPIC: (&str, &str) = (
    "handle-topic-epoch-pool-blocks",
    "rest.get.epochs.*.blocks.*",
);

// Assets topics
const DEFAULT_HANDLE_ASSETS_LIST_TOPIC: (&str, &str) =
    ("handle-topic-assets-list", "rest.get.assets");
const DEFAULT_HANDLE_ASSET_SINGLE_TOPIC: (&str, &str) = (
    "handle-topic-policy-assets-asset-single",
    "rest.get.assets.*",
);
const DEFAULT_HANDLE_ASSET_HISTORY_TOPIC: (&str, &str) =
    ("handle-topic-asset-history", "rest.get.assets.*.history");
const DEFAULT_HANDLE_ASSET_TRANSACTIONS_TOPIC: (&str, &str) = (
    "handle-topic-asset-transactions",
    "rest.get.assets.*.transactions",
);
const DEFAULT_HANDLE_ASSET_ADDRESSES_TOPIC: (&str, &str) = (
    "handle-topic-asset-addresses",
    "rest.get.assets.*.addresses",
);
const DEFAULT_HANDLE_POLICY_ASSETS_TOPIC: (&str, &str) =
    ("handle-topic-policy-assets", "rest.get.assets.policy.*");

#[module(
    message_type(Message),
    name = "rest-blockfrost",
    description = "Blockfrost-compatible REST API for Acropolis"
)]

pub struct BlockfrostREST;

impl BlockfrostREST {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // load query topics from config
        let handlers_config = Arc::new(HandlersConfig::from(config));

        info!("Blockfrost REST enabled");

        // Handler for /accounts/{stake_address}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_SINGLE_ACCOUNT_TOPIC,
            handlers_config.clone(),
            handle_single_account_blockfrost,
        );

        // Handler for /governance/dreps
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_DREPS_LIST_TOPIC,
            handlers_config.clone(),
            handle_dreps_list_blockfrost,
        );

        // Handler for /governance/dreps/{drep_id}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_SINGLE_DREP_TOPIC,
            handlers_config.clone(),
            handle_single_drep_blockfrost,
        );

        // Handler for /governance/dreps/{drep_id}/delegators
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_DREP_DELEGATORS_TOPIC,
            handlers_config.clone(),
            handle_drep_delegators_blockfrost,
        );

        // Handler for /governance/dreps/{drep_id}/metadata
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_DREP_METADATA_TOPIC,
            handlers_config.clone(),
            handle_drep_metadata_blockfrost,
        );

        // Handler for /governance/dreps/{drep_id}/updates
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_DREP_UPDATES_TOPIC,
            handlers_config.clone(),
            handle_drep_updates_blockfrost,
        );

        // Handler for /governance/dreps/{drep_id}/votes
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_DREP_VOTES_TOPIC,
            handlers_config.clone(),
            handle_drep_votes_blockfrost,
        );

        // Handler for /governance/proposals
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_PROPOSALS_LIST_TOPIC,
            handlers_config.clone(),
            handle_proposals_list_blockfrost,
        );

        // Handler for /governance/proposals/{tx_hash}/{cert_index}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_SINGLE_PROPOSAL_TOPIC,
            handlers_config.clone(),
            handle_single_proposal_blockfrost,
        );

        // Handler for /governance/proposals/{tx_hash}/{cert_index}/parameters
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_PROPOSAL_PARAMETERS_TOPIC,
            handlers_config.clone(),
            handle_proposal_parameters_blockfrost,
        );

        // Handler for /governance/proposals/{tx_hash}/{cert_index}/withdrawals
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_PROPOSAL_WITHDRAWALS_TOPIC,
            handlers_config.clone(),
            handle_proposal_withdrawals_blockfrost,
        );

        // Handler for /governance/proposals/{tx_hash}/{cert_index}/votes
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_PROPOSAL_VOTES_TOPIC,
            handlers_config.clone(),
            handle_proposal_votes_blockfrost,
        );

        // Handler for /governance/proposals/{tx_hash}/{cert_index}/metadata
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_PROPOSAL_METADATA_TOPIC,
            handlers_config.clone(),
            handle_proposal_metadata_blockfrost,
        );

        // Handler for /pools
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POOLS_LIST_TOPIC,
            handlers_config.clone(),
            handle_pools_list_blockfrost,
        );

        // Handler for /pools/extended, /pools/retired, /pools/retiring, and /pools/{pool_id}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POOLS_EXTENDED_RETIRED_RETIRING_SINGLE_TOPIC,
            handlers_config.clone(),
            handle_pools_extended_retired_retiring_single_blockfrost,
        );

        // Handler for /pools/{pool_id}/history
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POOL_HISTORY_TOPIC,
            handlers_config.clone(),
            handle_pool_history_blockfrost,
        );

        // Handler for /pools/{pool_id}/metadata
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POOL_METADATA_TOPIC,
            handlers_config.clone(),
            handle_pool_metadata_blockfrost,
        );

        // Handler for /pools/{pool_id}/relays
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POOL_RELAYS_TOPIC,
            handlers_config.clone(),
            handle_pool_relays_blockfrost,
        );

        // Handler for /pools/{pool_id}/delegators
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POOL_DELEGATORS_TOPIC,
            handlers_config.clone(),
            handle_pool_delegators_blockfrost,
        );

        // Handler for /pools/{pool_id}/blocks
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POOL_BLOCKS_TOPIC,
            handlers_config.clone(),
            handle_pool_blocks_blockfrost,
        );

        // Handler for /pools/{pool_id}/updates
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POOL_UPDATES_TOPIC,
            handlers_config.clone(),
            handle_pool_updates_blockfrost,
        );

        // Handler for /pools/{pool_id}/votes
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POOL_VOTES_TOPIC,
            handlers_config.clone(),
            handle_pool_votes_blockfrost,
        );

        // Handler for /epochs/latest and /epoches/{number}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_EPOCH_INFO_TOPIC,
            handlers_config.clone(),
            handle_epoch_info_blockfrost,
        );

        // Handler for /epochs/latest/parameters and /epochs/{number}/parameters
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_EPOCH_PARAMS_TOPIC,
            handlers_config.clone(),
            handle_epoch_params_blockfrost,
        );

        // Handler for /epochs/{number}/next
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_EPOCH_NEXT_TOPIC,
            handlers_config.clone(),
            handle_epoch_next_blockfrost,
        );

        // Handler for /epochs/{number}/previous
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_EPOCH_PREVIOUS_TOPIC,
            handlers_config.clone(),
            handle_epoch_previous_blockfrost,
        );

        // Handler for /epochs/{number}/stakes
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_EPOCH_TOTAL_STAKES_TOPIC,
            handlers_config.clone(),
            handle_epoch_total_stakes_blockfrost,
        );

        // Handler for /epochs/{number}/stakes/{pool_id}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_EPOCH_POOL_STAKES_TOPIC,
            handlers_config.clone(),
            handle_epoch_pool_stakes_blockfrost,
        );

        // Handler for /epochs/{number}/blocks
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_EPOCH_TOTAL_BLOCKS_TOPIC,
            handlers_config.clone(),
            handle_epoch_total_blocks_blockfrost,
        );

        // Handler for /epochs/{number}/blocks/{pool_id}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_EPOCH_POOL_BLOCKS_TOPIC,
            handlers_config.clone(),
            handle_epoch_pool_blocks_blockfrost,
        );

        // Handler for /assets
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_ASSETS_LIST_TOPIC,
            handlers_config.clone(),
            handle_assets_list_blockfrost,
        );

        // Handler for /assets/{asset}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_ASSET_SINGLE_TOPIC,
            handlers_config.clone(),
            handle_asset_single_blockfrost,
        );

        // Handler for /assets/{asset}/history
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_ASSET_HISTORY_TOPIC,
            handlers_config.clone(),
            handle_asset_history_blockfrost,
        );

        // Handler for /assets/{asset}/transactions
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_ASSET_TRANSACTIONS_TOPIC,
            handlers_config.clone(),
            handle_asset_transactions_blockfrost,
        );

        // Handler for /assets/{asset}/addresses
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_ASSET_ADDRESSES_TOPIC,
            handlers_config.clone(),
            handle_asset_addresses_blockfrost,
        );

        // Handler for /assets/policy/{policy_id}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_POLICY_ASSETS_TOPIC,
            handlers_config.clone(),
            handle_policy_assets_blockfrost,
        );

        Ok(())
    }
}

fn register_handler<F, Fut>(
    context: Arc<Context<Message>>,
    topic: (&str, &str),
    handlers_config: Arc<HandlersConfig>,
    handler_fn: F,
) where
    F: Fn(Arc<Context<Message>>, Vec<String>, Arc<HandlersConfig>) -> Fut
        + Send
        + Sync
        + Clone
        + 'static,
    Fut: Future<Output = Result<RESTResponse>> + Send + 'static,
{
    let topic_name = context.config.get_string(topic.0).unwrap_or_else(|_| topic.1.to_string());

    tracing::info!("Creating request handler on '{}'", topic_name);

    handle_rest_with_path_parameter(context.clone(), &topic_name, move |params| {
        let context = context.clone();
        let handler_fn = handler_fn.clone();
        let params: Vec<String> = params.iter().map(|s| s.to_string()).collect();
        let handlers_config = handlers_config.clone();

        async move { handler_fn(context, params, handlers_config).await }
    });
}
