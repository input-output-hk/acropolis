//! Acropolis Parameter State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::configuration::StartupMethod;
use acropolis_common::messages::{SnapshotMessage, SnapshotStateMessage, StateTransitionMessage};
use acropolis_common::queries::errors::QueryError;
use acropolis_common::{
    messages::{CardanoMessage, Message, ProtocolParamsMessage, StateQuery, StateQueryResponse},
    queries::parameters::{
        ParametersStateQuery, ParametersStateQueryResponse, DEFAULT_PARAMETERS_QUERY_TOPIC,
    },
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod alonzo_genesis;
mod genesis_params;
mod parameters_updater;
mod state;
use parameters_updater::ParametersUpdater;
use state::State;

const DEFAULT_ENACT_STATE_TOPIC: (&str, &str) = ("enact-state-topic", "cardano.enact.state");
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: (&str, &str) =
    ("publish-parameters-topic", "cardano.protocol.parameters");
const DEFAULT_NETWORK_NAME: (&str, &str) = ("network-name", "mainnet");
const DEFAULT_STORE_HISTORY: (&str, bool) = ("store-history", false);
/// Topic for receiving bootstrap data when starting from a CBOR dump snapshot
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

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
    pub protocol_parameters_topic: String,
    pub parameters_query_topic: String,
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
            protocol_parameters_topic: Self::conf(config, DEFAULT_PROTOCOL_PARAMETERS_TOPIC),
            parameters_query_topic: Self::conf(config, DEFAULT_PARAMETERS_QUERY_TOPIC),
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
        history: Arc<Mutex<StateHistory<State>>>,
        mut enact_s: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        // Process the snapshot messages first to bootstrap state if needed

        loop {
            let (_, message) = enact_s.read().await?;
            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::GovernanceOutcomes(gov))) => {
                    let span = info_span!("parameters_state.handle", epoch = block.epoch);
                    async {
                        // Get current state and current params
                        let mut state = {
                            let mut h = history.lock().await;
                            h.get_or_init_with(|| State::new(config.network_name.clone()))
                        };

                        // Handle rollback if needed
                        if block.status == BlockStatus::RolledBack {
                            state = history.lock().await.get_rolled_back_state(block.epoch);
                        }

                        if block.new_epoch {
                            // Get current params
                            let current_params = state.current_params.get_params();

                            // Process GovOutcomes message on era transition
                            let new_params = state.handle_enact_state(&block.era, gov).await?;

                            // Publish protocol params message
                            Self::publish_update(&config, block, new_params.clone())?;

                            // Commit state on params change
                            if current_params != new_params.params {
                                info!(
                                    "New parameter set enacted [from epoch, params]: [{},{}]",
                                    block.epoch,
                                    serde_json::to_string(&new_params.params)?
                                );
                                let mut h = history.lock().await;
                                h.commit(block.epoch, state);
                            }
                        }

                        Ok::<(), anyhow::Error>(())
                    }
                    .instrument(span)
                    .await?;
                }
                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    // forward the rollback downstream
                    config.context.publish(&config.protocol_parameters_topic, message).await?;
                }
                msg => error!("Unexpected message {msg:?} for enact state topic"),
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = ParametersStateConfig::new(context.clone(), &config);
        let enact = cfg.context.subscribe(&cfg.enact_state_topic).await?;
        let store_history = cfg.store_history;

        // Initalize state history
        let history = if store_history {
            Arc::new(Mutex::new(StateHistory::<State>::new(
                "ParameterState",
                StateHistoryStore::Unbounded,
            )))
        } else {
            Arc::new(Mutex::new(StateHistory::new(
                "ParameterState",
                StateHistoryStore::default_epoch_store(),
            )))
        };

        let query_state = history.clone();

        // Subscribe for snapshot messages, if booting from snapshot
        let snapshot_subscribe_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());

        let snapshot_subscription = if StartupMethod::from_config(config.as_ref()).is_snapshot() {
            info!("Creating subscriber on '{snapshot_subscribe_topic}'");
            Some(context.subscribe(&snapshot_subscribe_topic).await?)
        } else {
            None
        };

        let cfg_clone = cfg.clone();
        let history_clone = history.clone();
        let network_name = cfg.network_name.clone();

        if let Some(mut subscription) = snapshot_subscription {
            context.run(async move {
                loop {
                    let Ok((_, message)) = subscription.read().await else {
                        return;
                    };

                    match message.as_ref() {
                        Message::Snapshot(SnapshotMessage::Startup) => {
                            info!("ParameterState: Snapshot Startup message received");
                        }
                        Message::Snapshot(SnapshotMessage::Bootstrap(
                            SnapshotStateMessage::ParametersState(msg),
                        )) => {
                            // Get current state and current params
                            let mut state = {
                                let mut h = history.lock().await;
                                h.get_or_init_with(|| State::new(network_name.clone()))
                            };
                            info!("ParameterState: Snapshot Bootstrap message received");
                            match state.bootstrap(msg) {
                                Ok(epoch) => {
                                    let mut h = history.lock().await;
                                    h.commit(epoch, state);
                                }
                                Err(e) => {
                                    panic!("ParametersState bootstrap failed: {e}");
                                }
                            };
                        }
                        Message::Snapshot(SnapshotMessage::Complete) => {
                            info!("Snapshot complete, exiting Parameters state bootstrap loop");
                            break; // done processing snapshot messages
                        }
                        // There will be other snapshot messages that we're not interested in
                        _ => (),
                    }
                }
            });
        }

        // Handle parameters queries
        context.handle(&cfg.parameters_query_topic, move |message| {
            let history = query_state.clone();
            async move {
                let Message::StateQuery(StateQuery::Parameters(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Parameters(
                        ParametersStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for parameters-state",
                        )),
                    )));
                };

                let lock = history.lock().await;
                let response = match query {
                    ParametersStateQuery::GetLatestEpochParameters => {
                        ParametersStateQueryResponse::LatestEpochParameters(
                            lock.get_current_state().current_params.get_params(),
                        )
                    }
                    ParametersStateQuery::GetEpochParameters { epoch_number } => {
                        if !store_history {
                            ParametersStateQueryResponse::Error(QueryError::storage_disabled(
                                "Historical protocol parameter",
                            ))
                        } else {
                            match lock.get_at_or_before(*epoch_number) {
                                Some(state) => ParametersStateQueryResponse::EpochParameters(
                                    state.current_params.get_params(),
                                ),
                                None => ParametersStateQueryResponse::Error(QueryError::not_found(
                                    format!("Epoch {epoch_number} not found in history"),
                                )),
                            }
                        }
                    }
                    ParametersStateQuery::GetNetworkName => {
                        ParametersStateQueryResponse::NetworkName(
                            lock.get_current_state().network_name.clone(),
                        )
                    }
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Parameters(
                    response,
                )))
            }
        });

        // Start run task
        tokio::spawn(async move {
            Self::run(cfg_clone, history_clone, enact)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
