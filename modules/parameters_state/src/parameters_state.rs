//! Acropolis Parameter State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::{
    messages::{CardanoMessage, Message, ProtocolParamsMessage, StateQuery, StateQueryResponse},
    queries::epochs::{EpochsStateQuery, EpochsStateQueryResponse, LatestEpochParameters},
    rest_helper::{handle_rest, handle_rest_with_parameter},
    BlockInfo,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod alonzo_genesis;
mod genesis_params;
mod parameters_updater;
mod rest;
mod state;

use parameters_updater::ParametersUpdater;
use rest::handle_current;
use state::State;

use crate::rest::handle_historical;

const DEFAULT_ENACT_STATE_TOPIC: (&str, &str) = ("enact-state-topic", "cardano.enact.state");
const DEFAULT_HANDLE_CURRENT_TOPIC: (&str, &str) =
    ("handle-current-params-topic", "rest.get.epoch.parameters");
const DEFAULT_HANDLE_HISTORICAL_TOPIC: (&str, &str) = (
    "handle-historical-params-topic",
    "rest.get.epochs.*.parameters",
);
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: (&str, &str) =
    ("publish-parameters-topic", "cardano.protocol.parameters");
const DEFAULT_NETWORK_NAME: (&str, &str) = ("network-name", "mainnet");
const DEFAULT_STORE_HISTORY: (&str, bool) = ("store-history", false);

const PARAMETERS_STATE_TOPIC: &str = "parameters-state";

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
    pub handle_current_topic: String,
    pub handle_historical_topic: String,
    pub protocol_parameters_topic: String,
    pub store_history: bool,
}

impl ParametersStateConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!("Parameter value '{}' for {}", actual, keydef.0);
        actual
    }

    fn conf_bool(config: &Arc<Config>, keydef: (&str, bool)) -> bool {
        let actual = config.get_bool(keydef.0).unwrap_or(keydef.1);
        info!("Parameter value '{}' for {}", actual, keydef.0);
        actual
    }

    pub fn new(context: Arc<Context<Message>>, config: &Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            context,
            network_name: Self::conf(config, DEFAULT_NETWORK_NAME),
            enact_state_topic: Self::conf(config, DEFAULT_ENACT_STATE_TOPIC),
            handle_current_topic: Self::conf(config, DEFAULT_HANDLE_CURRENT_TOPIC),
            handle_historical_topic: Self::conf(config, DEFAULT_HANDLE_HISTORICAL_TOPIC),
            protocol_parameters_topic: Self::conf(config, DEFAULT_PROTOCOL_PARAMETERS_TOPIC),
            store_history: Self::conf_bool(config, DEFAULT_STORE_HISTORY),
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
        state: Arc<Mutex<State>>,
        mut enact_s: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        loop {
            match enact_s.read().await?.1.as_ref() {
                Message::Cardano((block, CardanoMessage::GovernanceOutcomes(gov))) => {
                    let span = info_span!("parameters_state.handle", block = block.number);
                    async {
                        let mut locked = state.lock().await;
                        let new_params = locked.handle_enact_state(&block, &gov).await?;
                        Self::publish_update(&config, &block, new_params)?;
                        Ok::<(), anyhow::Error>(())
                    }
                    .instrument(span)
                    .await?;
                }
                msg => error!("Unexpected message {msg:?} for enact state topic"),
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = ParametersStateConfig::new(context.clone(), &config);
        let enact = cfg.context.subscribe(&cfg.enact_state_topic).await?;
        let state = Arc::new(Mutex::new(State::new(
            cfg.network_name.clone(),
            cfg.store_history,
        )));

        let state_handle_current = state.clone();
        handle_rest(cfg.context.clone(), &cfg.handle_current_topic, move || {
            handle_current(state_handle_current.clone())
        });

        let state_handle_historical = state.clone();
        handle_rest_with_parameter(
            cfg.context.clone(),
            &cfg.handle_historical_topic,
            move |param| handle_historical(state_handle_historical.clone(), param[0].to_string()),
        );

        // Handle parameters state
        let state_rest_blockfrost = state.clone();
        context.handle(PARAMETERS_STATE_TOPIC, move |message| {
            let state = state_rest_blockfrost.clone();
            async move {
                let Message::StateQuery(StateQuery::Epochs(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Epochs(
                        EpochsStateQueryResponse::Error("Invalid message for epoch-state".into()),
                    )));
                };

                let state = state.lock().await;
                let response = match query {
                    EpochsStateQuery::GetLatestEpochParameters => {
                        EpochsStateQueryResponse::LatestEpochParameters(LatestEpochParameters {
                            parameters: state.current_params.get_params(),
                        })
                    }

                    _ => EpochsStateQueryResponse::Error(format!(
                        "Unimplemented query variant: {:?}",
                        query
                    )),
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Epochs(
                    response,
                )))
            }
        });

        // Start run task
        tokio::spawn(async move {
            Self::run(cfg, state, enact).await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
