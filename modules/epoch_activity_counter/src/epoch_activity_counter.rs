//! Acropolis epoch activity counter module for Caryatid
//! Unpacks block bodies to get transaction fees

use caryatid_sdk::{Context, Module, module, message_bus::Subscription};
use acropolis_common::{Era, messages::{Message, CardanoMessage}};
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
    /// Run loop
    async fn run(context: Arc<Context<Message>>, config: Arc<Config>,
                 mut headers_subscription: Box<dyn Subscription<Message>>,
                 mut fees_subscription: Box<dyn Subscription<Message>>) -> Result<()> {

        let publish_topic = config.get_string("publish-topic")
            .unwrap_or(DEFAULT_PUBLISH_TOPIC.to_string());
        info!("Publishing on '{publish_topic}'");

        // Create state
        // TODO!  Handling rollbacks with StateHistory
        let state = Arc::new(Mutex::new(State::new()));

        loop {
            // Read both topics in parallel
            let headers_message_f = headers_subscription.read();
            let fees_message_f = fees_subscription.read();

            // Handle headers first
            let (_, message) = headers_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::BlockHeader(header_msg))) => {

                    // End of epoch?
                    if block.new_epoch {
                        let mut state = state.lock().await;
                        let msg = state.end_epoch(&block, block.epoch-1);
                        context.message_bus.publish(&publish_topic, msg)
                            .await
                            .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                    }

                    // Derive the variant from the era - just enough to make
                    // MultiEraHeader::decode() work.
                    let variant = match block.era {
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
                                state.handle_mint(&block, vrf_vkey);
                            }
                        }

                        Err(e) => error!("Can't decode header {}: {e}", block.slot)
                    }
                }

                _ => error!("Unexpected message type: {message:?}")
            }

            // Handle block fees second - this is what generates the EpochActivity message
            let (_, message) = fees_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::BlockFees(fees_msg))) => {
                    let mut state = state.lock().await;
                    state.handle_fees(&block, fees_msg.total_fees);
                }

                _ => error!("Unexpected message type: {message:?}")
            }
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Get configuration
        let subscribe_headers_topic = config.get_string("subscribe-headers-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_HEADERS_TOPIC.to_string());
        info!("Creating subscriber for headers on '{subscribe_headers_topic}'");

        let subscribe_fees_topic = config.get_string("subscribe-fees-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_FEES_TOPIC.to_string());
        info!("Creating subscriber for fees on '{subscribe_fees_topic}'");

        // Subscribe
        let headers_subscription = context.message_bus.register(&subscribe_headers_topic).await?;
        let fees_subscription = context.message_bus.register(&subscribe_fees_topic).await?;

        // Start run task
        let run_context = context.clone();
        context.run(async move {
            Self::run(run_context, config, headers_subscription, fees_subscription)
                .await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
