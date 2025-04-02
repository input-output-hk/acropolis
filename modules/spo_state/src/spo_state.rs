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
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.spo-state.*";

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
        let state_handle = state.clone();
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

        // Handle requests for single SPO state
        context.message_bus.handle(&handle_topic, move |message: Arc<Message>| {
            let state = state_handle.clone();
            async move {
                let (code, body) = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        let lock = state.lock().await;
                        match request.path.rfind('/') {
                            None => (400, "Poorly formed url".to_string()),
                            Some(last_slash) => {
                                let id = &request.path[last_slash + 1..];
                                match id.len() {
                                    0 => match serde_json::to_string(lock.deref()) {
                                        Ok(body) => (200, body),
                                        Err(error) => (500, format!("{error:?}")),
                                    },
                                    _ => match hex::decode(&id) {
                                        Err(error) => (400, format!("SPO id must be hex encoded vector of bytes: {error:?}")),
                                        Ok(id) => match lock.deref().get(&id) {
                                            None => (404, "SPO not found".to_string()),
                                            Some(spo) => match serde_json::to_string(&spo) {
                                                Err(error) => (500, format!("{error:?}")),
                                                Ok(body) => (200, body),
                                            },
                                        },
                                    },
                                }
                            },
                        }
                    },
                    _ => {
                        error!("Unexpected message type {:?}", message);
                        ( 500, "Unexpected message in REST request".to_string())
                    }
                };

                Arc::new(Message::RESTResponse(RESTResponse { code, body,
                                                              content_type: None }))
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
