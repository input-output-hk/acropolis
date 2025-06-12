//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use acropolis_common::messages::{CardanoMessage, Message, RESTResponse, SnapshotStateMessage};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use serde_json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

mod state;

use state::State;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.spo-state";
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
        let subscribe_topic = config
            .get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let maybe_snapshot_topic = config
            .get_string("snapshot-topic")
            .ok()
            .inspect(|snapshot_topic| info!("Creating subscriber on '{snapshot_topic}'"));

        let handle_topic = config
            .get_string("handle-topic")
            .unwrap_or(DEFAULT_HANDLE_TOPIC.to_string());
        info!("Creating request handler on '{handle_topic}'");

        let spo_state_topic = config
            .get_string("publish-spo-state-topic")
            .unwrap_or(DEFAULT_SPO_STATE_TOPIC.to_string());
        info!("Creating SPO state publisher on '{spo_state_topic}'");

        let state = Arc::new(Mutex::new(State::new()));

        // Subscribe for snapshot messages, if allowed
        if let Some(snapshot_topic) = maybe_snapshot_topic {
            let mut subscription = context.message_bus.register(&snapshot_topic).await?;
            let state_subscription = state.clone();
            context.run(async move {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };

                match message.as_ref() {
                    Message::SnapshotState(SnapshotStateMessage::SPOState(spo_state)) => {
                        let mut state = state_subscription.lock().await;
                        state.bootstrap(spo_state.clone());
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
                    }
                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        // Handle requests for full SPO state
        let state_handle_full = state.clone();
        context.handle(&handle_topic, move |message: Arc<Message>| {
            let state = state_handle_full.clone();
            async move {
                let response = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        if let Some(state) = state.lock().await.current().clone() {
                            match serde_json::to_string(state) {
                                Ok(body) => RESTResponse::with_json(200, &body),
                                Err(error) => {
                                    RESTResponse::with_text(500, &format!("{error:?}").to_string())
                                }
                            }
                        } else {
                            RESTResponse::with_json(200, "{}")
                        }
                    }
                    _ => {
                        error!("Unexpected message type {:?}", message);
                        RESTResponse::with_text(500, "Unexpected message in REST request")
                    }
                };

                Arc::new(Message::RESTResponse(response))
            }
        });

        // Handle requests for single SPO state
        let handle_topic_single = handle_topic + ".*";
        let state_handle_single = state.clone();
        context.handle(&handle_topic_single, move |message: Arc<Message>| {
            let state = state_handle_single.clone();
            async move {
                let response = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        match request.path_elements.get(1) {
                            Some(id) => match hex::decode(&id) {
                                Ok(id) => {
                                    let state = state.lock().await;
                                    match state.get(&id) {
                                        Some(spo) => match serde_json::to_string(&spo) {
                                            Ok(body) => RESTResponse::with_json(200, &body),
                                            Err(error) => RESTResponse::with_text(500, &format!("{error:?}").to_string()),
                                        },
                                        None => RESTResponse::with_text(404, "SPO not found"),
                                    }
                                },
                                Err(error) => RESTResponse::with_text(400, &format!("SPO operator id must be hex encoded vector of bytes: {error:?}").to_string()),
                            },
                            None => RESTResponse::with_text(400, "SPO operator id must be provided"),
                        }
                    },
                    _ => {
                        error!("Unexpected message type {:?}", message);
                        RESTResponse::with_text(500, "Unexpected message in REST request")
                    }
                };

                Arc::new(Message::RESTResponse(response))
            }
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
                        state_tick
                            .lock()
                            .await
                            .tick()
                            .await
                            .inspect_err(|e| error!("Tick error: {e}"))
                            .ok();
                    }
                }
            }
        });

        Ok(())
    }
}
