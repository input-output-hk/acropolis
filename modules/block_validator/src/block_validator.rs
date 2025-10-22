//! Acropolis Block VRF Validator module for Caryatid
//! Validate the VRF calculation in the block header
use acropolis_common::messages::{CardanoMessage, Message, StateQuery, StateQueryResponse};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::sync::Arc;
use tracing::{error, info};
mod state;
use state::State;
mod ouroboros;

const DEFAULT_VALIDATION_VRF_PUBLISHER_TOPIC: (&str, &str) =
    ("validation-vrf-publisher-topic", "cardano.validation.vrf");
const DEFAULT_VALIDATION_KES_PUBLISHER_TOPIC: (&str, &str) =
    ("validation-kes-publisher-topic", "cardano.validation.kes");
const DEFAULT_BLOCK_HEADER_SUBSCRIBE_TOPIC: (&str, &str) =
    ("block-header-subscribe-topic", "cardano.block.header");
const DEFAULT_EPOCH_NONCES_SUBSCRIBE_TOPIC: (&str, &str) =
    ("epoch-nonces-subscribe-topic", "cardano.epoch.nonces");

/// Block Validator module
#[module(
    message_type(Message),
    name = "block-validator",
    description = "Validate the block header"
)]

pub struct BlockValidator;

impl BlockValidator {
    async fn run() -> Result<()> {
        Ok(())
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Publish topics
        let validation_vrf_publisher_topic = config
            .get_string(DEFAULT_VALIDATION_VRF_PUBLISHER_TOPIC.0)
            .unwrap_or(DEFAULT_VALIDATION_VRF_PUBLISHER_TOPIC.1.to_string());
        info!("Creating validation VRF publisher on '{validation_vrf_publisher_topic}'");

        // Subscribe topics
        let block_headers_subscribe_topic = config
            .get_string(DEFAULT_BLOCK_HEADER_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCK_HEADER_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating block headers subscription on '{block_headers_subscribe_topic}'");

        let epoch_nonces_subscribe_topic = config
            .get_string(DEFAULT_EPOCH_NONCES_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCH_NONCES_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating epoch nonces subscription on '{epoch_nonces_subscribe_topic}'");

        // Subscribers
        let block_headers_subscription = context.subscribe(&block_headers_subscribe_topic).await?;
        let epoch_nonces_subscription = context.subscribe(&epoch_nonces_subscribe_topic).await?;

        // Start run task
        context.run(async move {
            Self::run().await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
