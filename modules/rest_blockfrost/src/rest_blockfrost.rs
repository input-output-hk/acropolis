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
mod handlers;
mod handlers_config;
mod types;
mod utils;
use handlers::{
    accounts::handle_single_account_blockfrost,
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
    handlers::epochs::{handle_latest_epoch_blockfrost, handle_single_epoch_blockfrost},
    handlers_config::HandlersConfig,
};

// Accounts topics
const DEFAULT_HANDLE_SINGLE_ACCOUNT_TOPIC: (&str, &str) =
    ("handle-topic-account-single", "rest.get.accounts.*");

// Epochs topics
const DEFAULT_HANDLE_LATEST_EPOCH_TOPIC: (&str, &str) =
    ("handle-topic-epoch-latest", "rest.get.epoch");
const DEFAULT_HANDLE_SINGLE_EPOCH_TOPIC: (&str, &str) =
    ("handle-topic-epoch-single", "rest.get.epochs.*");

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

        // Handler for /epochs
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_LATEST_EPOCH_TOPIC,
            handlers_config.clone(),
            handle_latest_epoch_blockfrost,
        );

        // Handler for /epochs/{epoch}
        register_handler(
            context.clone(),
            DEFAULT_HANDLE_SINGLE_EPOCH_TOPIC,
            handlers_config.clone(),
            handle_single_epoch_blockfrost,
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
