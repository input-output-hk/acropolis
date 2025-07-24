//! Acropolis Blockfrost-Compatible REST Module

use std::sync::Arc;

use acropolis_common::{messages::Message, rest_helper::handle_rest_with_parameter};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use tracing::info;
mod handlers;
use handlers::accounts::handle_single_account_blockfrost;

const DEFAULT_HANDLE_SINGLE_ACCOUNT_TOPIC: (&str, &str) =
    ("handle-topic-account-single", "rest.get.accounts.*");

#[module(
    message_type(Message),
    name = "rest-blockfrost",
    description = "Blockfrost-compatible REST API for Acropolis"
)]

pub struct BlockfrostREST;

impl BlockfrostREST {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        info!("Blockfrost REST enabled");
        // Register routes with the shared REST server
        let handle_single_account_topic = config
            .get_string(DEFAULT_HANDLE_SINGLE_ACCOUNT_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_SINGLE_ACCOUNT_TOPIC.1.to_string());

        info!(
            "Creating request handler on '{}'",
            handle_single_account_topic
        );

        // Register individual endpoint handlers
        handle_rest_with_parameter(
            context.clone(),
            &handle_single_account_topic,
            move |param| handle_single_account_blockfrost(context.clone(), param[0].to_string()),
        );

        // Add more routes here as needed

        Ok(())
    }
}
