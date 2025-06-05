//! Acropolis Parameter State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt, message_bus::Subscription};
use acropolis_common::{BlockInfo, messages::{Message, RESTResponse, CardanoMessage, ProtocolParamsMessage}};
use std::sync::Arc;
use anyhow::{anyhow, Result};
use config::Config;
use tokio::sync::Mutex;
use tracing::{error, info};

mod state;
mod parameters_updater;

use state::State;
use parameters_updater::ParametersUpdater;

const DEFAULT_ENACT_STATE_TOPIC: (&str,&str) = ("enact-state-topic", "cardano.enact.state");
const DEFAULT_GENESIS_COMPLETE_TOPIC: (&str,&str) = ("genesis-complete-topic", "cardano.sequence.bootstrapped");
const DEFAULT_HANDLE_TOPIC: (&str,&str) = ("handle-topic", "rest.get.governance-state.*");
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: (&str,&str) = ("publish-parameters-topic", "cardano.protocol.parameters");

/// Parameters State module 
#[module(
    message_type(Message),
    name = "parameters-state",
    description = "Current protocol parameters handling"
)]
pub struct ParametersState;

struct ParametersStateConfig {
    pub context: Arc<Context<Message>>,
    pub enact_state_topic: String,
    pub genesis_complete_topic: String,
    pub handle_topic: String,
    pub protocol_parameters_topic: String
}

impl ParametersStateConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!("Creating subscriber on '{}' for {}", actual, keydef.0);
        actual
    }

    pub fn new(context: Arc<Context<Message>>, config: &Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            context,
            enact_state_topic: Self::conf(config, DEFAULT_ENACT_STATE_TOPIC),
            genesis_complete_topic: Self::conf(config, DEFAULT_GENESIS_COMPLETE_TOPIC),
            handle_topic: Self::conf(config, DEFAULT_HANDLE_TOPIC),
            protocol_parameters_topic: Self::conf(config, DEFAULT_PROTOCOL_PARAMETERS_TOPIC)
        })
    }
}

impl ParametersState
{
    fn publish_update(
        config: &Arc<ParametersStateConfig>,
        block: &BlockInfo,
        message: ProtocolParamsMessage
    ) -> Result<()> {
        let config = config.clone();
        let packed_message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::ProtocolParams(message)
        )));

        info!("Publishing {:?} at {}", packed_message, config.protocol_parameters_topic);
        tokio::spawn(async move {
            config.context.message_bus
                .publish(&config.protocol_parameters_topic, packed_message).await
                .unwrap_or_else(|e| tracing::error!("Failed to publish: {e}")); 
            info!("Published");
        });

        Ok(())
    }

    async fn run(config: Arc<ParametersStateConfig>,
                 mut genesis_s: Box<dyn Subscription<Message>>,
                 mut enact_s: Box<dyn Subscription<Message>>
    ) -> Result<()> {
        let state = Arc::new(Mutex::new(State::new()));

        match &genesis_s.read().await?.1.as_ref() {
            Message::Cardano((block, CardanoMessage::GenesisComplete(genesis))) => {
                state.lock().await.handle_genesis(&genesis).await?;
            },
            msg => return Err(anyhow!("Unexpected genesis {msg:?}; cannot initialize parameters module"))
        };
        info!("genesis complete");

        loop {
            match enact_s.read().await?.1.as_ref() {
                Message::Cardano((block, CardanoMessage::EnactState(enact))) => {
                    let params = state.lock().await.handle_enact_state(&block, &enact).await?;
                    info!("enact state {:?}", block);
                    Self::publish_update(&config, &block, params)?;
                },
                msg => error!("Unexpected message {msg:?} for enact state topic")
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = ParametersStateConfig::new(context.clone(), &config);
        let genesis = cfg.context.message_bus.register(&cfg.genesis_complete_topic).await?;
        let enact = cfg.context.message_bus.register(&cfg.enact_state_topic).await?;

        // Start run task
        tokio::spawn(async move {
            Self::run(cfg, genesis, enact).await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
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
}
