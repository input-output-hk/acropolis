//! Acropolis Parameter State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::{
    messages::{CardanoMessage, Message, ProtocolParamsMessage, RESTResponse},
    BlockInfo,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

mod genesis_params;
mod parameters_updater;
mod state;

use parameters_updater::ParametersUpdater;
use state::State;

const DEFAULT_ENACT_STATE_TOPIC: (&str, &str) = ("enact-state-topic", "cardano.enact.state");
const DEFAULT_HANDLE_TOPIC: (&str, &str) = ("handle-topic", "rest.get.governance-state.*");
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: (&str, &str) =
    ("publish-parameters-topic", "cardano.protocol.parameters");
const DEFAULT_NETWORK_NAME: (&str, &str) = ("network-name", "mainnet");

/// Parameters State module
#[module(
    message_type(Message),
    name = "parameters-state",
    description = "Current protocol parameters handling"
)]
pub struct ParametersState;

struct ParametersStateConfig {
    pub context: Arc<Context<Message>>,
    pub network_name: String,
    pub enact_state_topic: String,
    pub handle_topic: String,
    pub protocol_parameters_topic: String,
}

impl ParametersStateConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!("Parameter value '{}' for {}", actual, keydef.0);
        actual
    }

    pub fn new(context: Arc<Context<Message>>, config: &Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            context,
            network_name: Self::conf(config, DEFAULT_NETWORK_NAME),
            enact_state_topic: Self::conf(config, DEFAULT_ENACT_STATE_TOPIC),
            handle_topic: Self::conf(config, DEFAULT_HANDLE_TOPIC),
            protocol_parameters_topic: Self::conf(config, DEFAULT_PROTOCOL_PARAMETERS_TOPIC),
        })
    }
}

impl ParametersState {
    fn publish_update(
        config: &Arc<ParametersStateConfig>,
        block: &BlockInfo,
        message: ProtocolParamsMessage,
    ) -> Result<()> {
        let config = config.clone();
        let packed_message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::ProtocolParams(message),
        )));

        tokio::spawn(async move {
            config
                .context
                .publish(&config.protocol_parameters_topic, packed_message)
                .await
                .unwrap_or_else(|e| tracing::error!("Failed to publish: {e}"));
        });

        Ok(())
    }

    async fn run(
        config: Arc<ParametersStateConfig>,
        mut enact_s: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let state = Arc::new(Mutex::new(State::new(config.network_name.clone())));
        let state_handle = state.clone();

        config.context.handle(&config.handle_topic, move |message: Arc<Message>| {
            let _state = state_handle.clone();
            async move {
                let response = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        RESTResponse::with_text(200, "Ok")
                        /*
                        let lock = state.lock().await;

                        match perform_rest_request(&lock, &request.path) {
                            Ok(response) => RESTResponse::with_text(200, &response),
                            Err(error) => {
                                error!("Governance State REST request error: {error:?}");
                                RESTResponse::with_text(400, &format!("{error:?}"))
                            }
                        }
                        */
                    }
                    _ => {
                        error!("Unexpected message type: {message:?}");
                        RESTResponse::with_text(500, &format!("Unexpected message type"))
                    }
                };

                Arc::new(Message::RESTResponse(response))
            }
        });

        loop {
            info!("Waiting for enact-state");
            match enact_s.read().await?.1.as_ref() {
                Message::Cardano((block, CardanoMessage::GovernanceOutcomes(gov))) => {
                    let mut locked = state.lock().await;
                    let new_params = locked.handle_enact_state(&block, &gov).await?;
                    Self::publish_update(&config, &block, new_params)?;
                }
                msg => error!("Unexpected message {msg:?} for enact state topic"),
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = ParametersStateConfig::new(context.clone(), &config);
        let enact = cfg.context.subscribe(&cfg.enact_state_topic).await?;

        // Start run task
        tokio::spawn(async move {
            Self::run(cfg, enact).await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
