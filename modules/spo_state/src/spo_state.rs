//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use acropolis_common::{
    messages::{
        CardanoMessage, Message, SnapshotDumpMessage, SnapshotMessage, SnapshotStateMessage,
    },
    rest_helper::{handle_rest, handle_rest_with_parameter},
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod state;
use state::State;
mod rest;
use rest::{handle_list, handle_retiring_pools, handle_spo};

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const DEFAULT_LIST_TOPIC: (&str, &str) = ("handle-topic-pool-list", "rest.get.pools");
const DEFAULT_SINGLE_TOPIC: (&str, &str) = ("handle-topic-pool-info", "rest.get.pools.*");
const DEFAULT_RETIRING_POOLS_TOPIC: (&str, &str) =
    ("handle-topic-retiring-pools", "rest.get.pools_retiring");
const DEFAULT_SPO_STATE_TOPIC: &str = "cardano.spo.state";

/// SPO State module
#[module(
    message_type(Message),
    name = "spo-state",
    description = "In-memory SPO State from certificate events"
)]
pub struct SPOState;

impl SPOState {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let maybe_snapshot_topic = config
            .get_string("snapshot-topic")
            .ok()
            .inspect(|snapshot_topic| info!("Creating subscriber on '{snapshot_topic}'"));

        let handle_list_topic =
            config.get_string(DEFAULT_LIST_TOPIC.0).unwrap_or(DEFAULT_LIST_TOPIC.1.to_string());
        info!("Creating request handler on '{handle_list_topic}'");

        let handle_single_topic =
            config.get_string(DEFAULT_SINGLE_TOPIC.0).unwrap_or(DEFAULT_SINGLE_TOPIC.1.to_string());
        info!("Creating request handler on '{handle_single_topic}'");

        let handle_retiring_pools_topic = config
            .get_string(DEFAULT_RETIRING_POOLS_TOPIC.0)
            .unwrap_or(DEFAULT_RETIRING_POOLS_TOPIC.1.to_string());
        info!("Creating request handler on '{handle_retiring_pools_topic}'");

        let spo_state_topic = config
            .get_string("publish-spo-state-topic")
            .unwrap_or(DEFAULT_SPO_STATE_TOPIC.to_string());
        info!("Creating SPO state publisher on '{spo_state_topic}'");

        let state = Arc::new(Mutex::new(State::new()));

        // Subscribe for snapshot messages, if allowed
        if let Some(snapshot_topic) = maybe_snapshot_topic {
            let mut subscription = context.subscribe(&snapshot_topic).await?;
            let context_snapshot = context.clone();
            let state_snapshot = state.clone();
            context.run(async move {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };

                match message.as_ref() {
                    Message::Snapshot(SnapshotMessage::Bootstrap(
                        SnapshotStateMessage::SPOState(spo_state),
                    )) => {
                        let mut state = state_snapshot.lock().await;
                        state.bootstrap(spo_state.clone());
                    }
                    Message::Snapshot(SnapshotMessage::DumpRequest(SnapshotDumpMessage {
                        block_height,
                    })) => {
                        info!("inspecting state at block height {}", block_height);
                        let state = state_snapshot.lock().await;
                        let maybe_spo_state = state.dump(*block_height);

                        if let Some(spo_state) = maybe_spo_state {
                            context_snapshot
                                .message_bus
                                .publish(
                                    &snapshot_topic,
                                    Arc::new(Message::Snapshot(SnapshotMessage::Dump(
                                        SnapshotStateMessage::SPOState(spo_state),
                                    ))),
                                )
                                .await
                                .unwrap_or_else(|e| error!("failed to publish snapshot dump: {e}"))
                        }
                    }
                    _ => error!("Unexpected message type: {message:?}"),
                }
            });
        }

        // Subscribe for certificate messages
        let mut subscription = context.subscribe(&subscribe_topic).await?;
        let context_subscribe = context.clone();
        let state_subscribe = state.clone();
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };

                match message.as_ref() {
                    Message::Cardano((block, CardanoMessage::TxCertificates(tx_certs_msg))) => {
                        let span = info_span!("spo_state.handle_tx_certs", block = block.number);
                        async {
                            // End of epoch?
                            if block.new_epoch && block.epoch > 0 {
                                let mut state = state_subscribe.lock().await;
                                let msg = state.end_epoch(&block);
                                context_subscribe
                                    .message_bus
                                    .publish(&spo_state_topic, msg)
                                    .await
                                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                            }

                            let mut state = state_subscribe.lock().await;
                            state
                                .handle_tx_certs(block, tx_certs_msg)
                                .inspect_err(|e| error!("Messaging handling error: {e}"))
                                .ok();
                        }.instrument(span).await;
                    }
                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        // Handle REST requests for full SPO state
        let state_list = state.clone();
        handle_rest(context.clone(), &handle_list_topic, move || {
            handle_list(state_list.clone())
        });

        let state_single = state.clone();
        handle_rest_with_parameter(context.clone(), &handle_single_topic, move |param| {
            handle_spo(state_single.clone(), param[0].to_string())
        });

        // Handle REST requests for retiring pools
        let state_retiring_pools = state.clone();
        handle_rest(context.clone(), &handle_retiring_pools_topic, move || {
            handle_retiring_pools(state_retiring_pools.clone())
        });

        // Ticker to log stats
        let state_tick = state.clone();
        let mut subscription = context.subscribe("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("spo_state.tick", number = message.number);
                        async {
                            state_tick
                                .lock()
                                .await
                                .tick()
                                .await
                                .inspect_err(|e| error!("Tick error: {e}"))
                                .ok();
                        }.instrument(span).await;
                    }
                }
            }
        });

        Ok(())
    }
}
