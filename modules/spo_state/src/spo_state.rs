//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{
    messages::{Message, RESTResponse},
    Serialiser,
};
use std::ops::Deref;
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tokio::sync::Mutex;
use tracing::{error, info};
use serde_json;

mod state;
use state::State;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.spo-state";

/// SPO State module
#[module(
    message_type(Message),
    name = "spo-state",
    description = "In-memory SPO State from certificate events"
)]
pub struct SPOState;

impl SPOState
{
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let handle_topic = config.get_string("handle-topic")
            .unwrap_or(DEFAULT_HANDLE_TOPIC.to_string());
        info!("Creating request handler on '{handle_topic}'");

        let state = Arc::new(Mutex::new(State::new()));
        let state_handle_full = state.clone();
        let state_handle_single = state.clone();
        let state_tick = state.clone();

        let serialiser = Arc::new(Mutex::new(Serialiser::new(state, module_path!(), 1)));
        let serialiser_tick = serialiser.clone();


        // Subscribe for certificate messages
        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let serialiser = serialiser.clone();
            async move {
                match message.as_ref() {
                    Message::TxCertificates(tx_cert_msg) => {
                        let mut serialiser = serialiser.lock().await;
                        serialiser.handle_message(tx_cert_msg.sequence, tx_cert_msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Handle requests for full SPO state
        context.message_bus.handle(&handle_topic, move |message: Arc<Message>| {
            let state = state_handle_full.clone();
            async move {
                let response = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        let lock = state.lock().await;
                        match serde_json::to_string(lock.deref()) {
                            Ok(body) => RESTResponse::with_json(200, &body),
                            Err(error) => RESTResponse::with_text(500, &format!("{error:?}").to_string()),
                        }
                    },
                    _ => {
                        error!("Unexpected message type {:?}", message);
                        RESTResponse::with_text(500, "Unexpected message in REST request")
                    }
                };

                Arc::new(Message::RESTResponse(response))
            }
        })?;

        let handle_topic_single = handle_topic + ".*";

        // Handle requests for single SPO state
        context.message_bus.handle(&handle_topic_single, move |message: Arc<Message>| {
            let state = state_handle_single.clone();
            async move {
                let response = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        match request.path_elements.get(1) {
                            Some(id) => match hex::decode(&id) {
                                Ok(id) => {
                                    let lock = state.lock().await;
                                    match lock.deref().get(&id) {
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
        })?;

        // Ticker to log stats
        context.clone().message_bus.subscribe("clock.tick", move |message: Arc<Message>| {
            let serialiser = serialiser_tick.clone();
            let state = state_tick.clone();

            async move {
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state.lock().await.tick()
                            .await
                            .inspect_err(|e| error!("Tick error: {e}"))
                            .ok();
                        serialiser.lock().await.tick();
                    }
                }
            }
        })?;

        Ok(())
    }
}
