//! Acropolis Parameter State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt, message_bus::Subscription};
use acropolis_common::messages::{Message, RESTResponse, CardanoMessage};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tokio::sync::Mutex;
use tracing::{error, info};

mod state;
mod parameters_updater;

use state::State;
use parameters_updater::ParametersUpdater;

const DEFAULT_ENACT_STATE_TOPIC: &str = "cardano.enact.state";
const DEFAULT_GENESIS_COMPLETE_TOPIC: &str = "cardano.sequence.bootstrapped";
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.governance-state.*";
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: &str = "cardano.protocol.parameters";

/// Parameters State module
#[module(
    message_type(Message),
    name = "parameters-state",
    description = "Current protocol parameters handling"
)]
pub struct ParametersState;

impl ParametersState
{
    async fn run(context: Arc<Context<Message>>,
                 mut genesis_s: Box<dyn Subscription<Message>>,
                 mut enact_s: Box<dyn Subscription<Message>>) -> Result<()>
    {
        let state = Arc::new(Mutex::new(State::new()));

        {
            match &genesis_s.read().await?.1.as_ref() {
                Message::Cardano((_block, CardanoMessage::GenesisComplete(genesis))) => 
                    state.lock().await.handle_genesis(&genesis).await?,
                msg => error!("Unexpected message {msg:?} for genesis topic")
            }
        }

        loop {
            match enact_s.read().await?.1.as_ref() {
                Message::Cardano((block, CardanoMessage::EnactState(enact))) => 
                    state.lock().await.handle_enact_state(&block, &enact).await?,
                msg => error!("Unexpected message {msg:?} for enact state topic")
            }
        }
    }

    async fn async_init(context: Arc<Context<Message>>, genesis_topic: String, enact_topic: String) -> Result<()> {
        let genesis = context.message_bus.register(&genesis_topic).await?;
        let enact = context.message_bus.register(&enact_topic).await?;

        // Start run task
        tokio::spawn(async move {
            Self::run(context, genesis, enact)
                .await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }

    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
/*
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let handle_topic = config.get_string("handle-topic")
            .unwrap_or(DEFAULT_HANDLE_TOPIC.to_string());
        info!("Creating request handler on '{handle_topic}'");
 */
        let enact_topic = config.get_string("enact-state-topic")
            .unwrap_or(DEFAULT_ENACT_STATE_TOPIC.to_string());
        info!("Creating request handler on '{enact_topic}'");

        let genesis_topic = config.get_string("genesis-complete-topic")
            .unwrap_or(DEFAULT_GENESIS_COMPLETE_TOPIC.to_string());
        info!("Creating request handler on '{genesis_topic}'");

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                Self::async_init(context, genesis_topic, enact_topic)
                    .await.unwrap_or_else(|e| error!("Failed: {e}"));
            })
        });
/*
        // Subscribe to governance procedures serializer
        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let state = state_enact.clone();

            async move {
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::EnactState(msg))) => {
                        let mut state = state.lock().await;
                        state.handle_enact_state(block_info, msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Subscribe to bootstrap completion serializer
        context.clone().message_bus.subscribe(&genesis_complete_topic, move |message: Arc<Message>| {
            let state = state_genesis.clone();

            async move {
                match message.as_ref() {
                    Message::Cardano((_block_info, CardanoMessage::GenesisComplete(msg))) => {
                        let mut state = state.lock().await;
                        state.handle_genesis(msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;
*/
        // REST requests handling
/*
        context.message_bus.handle(&handle_topic, move |message: Arc<Message>| {
            let state = state_handle.clone();
            async move {
                let response = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        let lock = state.lock().await;

                        match perform_rest_request(&lock, &request.path) {
                            Ok(response) => RESTResponse::with_text(200, &response),
                            Err(error) => {
                                error!("Governance State REST request error: {error:?}");
                                RESTResponse::with_text(400, &format!("{error:?}"))
                            }
                        }
                    },
                    _ => {
                        error!("Unexpected message type: {message:?}");
                        RESTResponse::with_text(500, &format!("Unexpected message type"))
                    }
                };

                Arc::new(Message::RESTResponse(response))
            }
        })?;
*/
        // Ticker to log stats
/*
        context.clone().message_bus.subscribe("clock.tick", move |message: Arc<Message>| {
            let state = state_tick.clone();

            async move {
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state.lock().await.tick()
                            .await
                            .inspect_err(|e| error!("Tick error: {e}"))
                            .ok();
                    }
                }
            }
        })?;
*/
        Ok(())
    }
}
