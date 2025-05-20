//! Acropolis epoch activity counter module for Caryatid
//! Unpacks block bodies to get transaction fees

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{Era, messages::Message};
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use config::Config;
use tracing::{info, error};
use pallas::ledger::traverse::MultiEraHeader;

mod state;
use state::State;

const DEFAULT_SUBSCRIBE_HEADERS_TOPIC: &str = "cardano.block.header";
const DEFAULT_SUBSCRIBE_FEES_TOPIC: &str = "cardano.block.fees";
const DEFAULT_PUBLISH_TOPIC: &str = "cardano.epoch.activity";

/// Epoch activity counter module
#[module(
    message_type(Message),
    name = "epoch-activity-counter",
    description = "Epoch activity counter"
)]
pub struct EpochActivityCounter;

impl EpochActivityCounter
{
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Get configuration
        let subscribe_headers_topic = config.get_string("subscribe-headers-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_HEADERS_TOPIC.to_string());
        info!("Creating subscriber for headers on '{subscribe_headers_topic}'");

        let subscribe_fees_topic = config.get_string("subscribe-fees-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_FEES_TOPIC.to_string());
        info!("Creating subscriber for fees on '{subscribe_fees_topic}'");

        let publish_topic = config.get_string("publish-topic")
            .unwrap_or(DEFAULT_PUBLISH_TOPIC.to_string());
        info!("Publishing on '{publish_topic}'");

        // Create state
        let state = Arc::new(Mutex::new(State::new()));
        let state_headers = state.clone();
        let state_fees = state.clone();

        // TODO!  Synchronisation between these two subscriptions - fees may be wrongly
        // accounted to the next epoch if the order is wrong

        // TODO!  Handling rollbacks - delay by 'k' is an option

        // Handle block headers
        let context_headers = context.clone();
        context.clone().message_bus.subscribe(&subscribe_headers_topic,
                                              move |message: Arc<Message>| {
            let state = state_headers.clone();
            let publish_topic = publish_topic.clone();
            let context = context_headers.clone();

            async move {
                match message.as_ref() {
                    Message::BlockHeader(header_msg) => {

                        // End of epoch?
                        if header_msg.block.new_epoch {
                            let mut state = state.lock().await;
                            let msg = state.end_epoch(&header_msg.block);
                            context.message_bus.publish(&publish_topic, msg)
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                        }

                        // Derive the variant from the era - just enough to make
                        // MultiEraHeader::decode() work.
                        let variant = match header_msg.block.era {
                            Era::Byron => 0,
                            Era::Shelley => 1,
                            Era::Allegra => 2,
                            Era::Mary => 3,
                            Era::Alonzo => 4,
                            _ => 5,
                        };

                        // Parse the header - note we ignore the subtag because EBBs
                        // are suppressed upstream
                        match MultiEraHeader::decode(variant, None, &header_msg.raw) {
                            Ok(header) => {
                                if let Some(vrf_vkey) = header.vrf_vkey() {
                                    let mut state = state.lock().await;
                                    state.handle_mint(&header_msg.block, vrf_vkey);
                                }
                            }

                            Err(e) => error!("Can't decode header {}: {e}", header_msg.block.slot)
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Handle block fees
        context.clone().message_bus.subscribe(&subscribe_fees_topic,
                                              move |message: Arc<Message>| {
            let state = state_fees.clone();
            async move {
                match message.as_ref() {
                    Message::BlockFees(fees_msg) => {
                        let mut state = state.lock().await;
                        state.handle_fees(&fees_msg.block, fees_msg.total_fees);
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        Ok(())
    }
}
