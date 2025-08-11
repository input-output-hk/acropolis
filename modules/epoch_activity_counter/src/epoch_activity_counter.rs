//! Acropolis epoch activity counter module for Caryatid
//! Unpacks block bodies to get transaction fees

use acropolis_common::{
    messages::{CardanoMessage, Message},
    rest_helper::{handle_rest, handle_rest_with_parameter},
    Era,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context, Module};
use config::Config;
use pallas::ledger::traverse::MultiEraHeader;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod state;
use state::State;
mod rest;
use rest::{handle_epoch, handle_historical_epoch};

const DEFAULT_SUBSCRIBE_HEADERS_TOPIC: &str = "cardano.block.header";
const DEFAULT_SUBSCRIBE_FEES_TOPIC: &str = "cardano.block.fees";
const DEFAULT_PUBLISH_TOPIC: &str = "cardano.epoch.activity";
const DEFAULT_HANDLE_CURRENT_TOPIC: (&str, &str) = ("handle-topic-current-epoch", "rest.get.epoch");
const DEFAULT_HANDLE_HISTORICAL_TOPIC: (&str, &str) =
    ("handle-topic-historical-epoch", "rest.get.epochs.*");
const DEFAULT_STORE_HISTORY: (&str, bool) = ("store-history", false);

/// Epoch activity counter module
#[module(
    message_type(Message),
    name = "epoch-activity-counter",
    description = "Epoch activity counter"
)]
pub struct EpochActivityCounter;

impl EpochActivityCounter {
    /// Run loop
    async fn run(
        context: Arc<Context<Message>>,
        config: Arc<Config>,
        state: Arc<Mutex<State>>,
        mut headers_subscription: Box<dyn Subscription<Message>>,
        mut fees_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let publish_topic =
            config.get_string("publish-topic").unwrap_or(DEFAULT_PUBLISH_TOPIC.to_string());
        info!("Publishing on '{publish_topic}'");

        loop {
            // Read both topics in parallel
            let headers_message_f = headers_subscription.read();
            let fees_message_f = fees_subscription.read();

            // Handle headers first
            let (_, message) = headers_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::BlockHeader(header_msg))) => {
                    let span = info_span!(
                        "epoch_activity_counter.handle_block_header",
                        block = block.number
                    );
                    async {
                        // End of epoch?
                        if block.new_epoch && block.epoch > 0 {
                            let mut state = state.lock().await;
                            let msg = state.end_epoch(&block, block.epoch - 1);
                            context
                                .message_bus
                                .publish(&publish_topic, msg)
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
                                    state.handle_mint(&block, Some(vrf_vkey));
                                }
                            }

                            Err(e) => error!("Can't decode header {}: {e}", block.slot),
                        }
                    }
                    .instrument(span)
                    .await;
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Handle block fees second so new epoch's fees don't get counted in the last one
            let (_, message) = fees_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::BlockFees(fees_msg))) => {
                    let span = info_span!(
                        "epoch_activity_counter.handle_block_fees",
                        block = block.number
                    );
                    async {
                        let mut state = state.lock().await;
                        state.handle_fees(&block, fees_msg.total_fees);
                    }
                    .instrument(span)
                    .await;
                }

                _ => error!("Unexpected message type: {message:?}"),
            }
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_headers_topic = config
            .get_string("subscribe-headers-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_HEADERS_TOPIC.to_string());
        info!("Creating subscriber for headers on '{subscribe_headers_topic}'");

        let subscribe_fees_topic = config
            .get_string("subscribe-fees-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_FEES_TOPIC.to_string());
        info!("Creating subscriber for fees on '{subscribe_fees_topic}'");

        let store_history =
            config.get_bool(DEFAULT_STORE_HISTORY.0).unwrap_or(DEFAULT_STORE_HISTORY.1);

        // REST handler topics
        let handle_current_topic = config
            .get_string(DEFAULT_HANDLE_CURRENT_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_CURRENT_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_current_topic);

        let handle_historical_topic = config
            .get_string(DEFAULT_HANDLE_HISTORICAL_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_HISTORICAL_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_historical_topic);

        // Subscribe
        let headers_subscription = context.subscribe(&subscribe_headers_topic).await?;
        let fees_subscription = context.subscribe(&subscribe_fees_topic).await?;

        // Create state
        // TODO!  Handling rollbacks with StateHistory
        let state = Arc::new(Mutex::new(State::new(store_history)));

        handle_rest(context.clone(), &handle_current_topic, {
            let state = state.clone();
            move || {
                let state = state.clone();
                async move { handle_epoch(state).await }
            }
        });

        handle_rest_with_parameter(context.clone(), &handle_historical_topic, {
            let state = state.clone();
            move |param| handle_historical_epoch(state.clone(), param[0].to_string())
        });

        // Start run task
        let run_context = context.clone();
        context.run(async move {
            Self::run(
                run_context,
                config,
                state,
                headers_subscription,
                fees_subscription,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
