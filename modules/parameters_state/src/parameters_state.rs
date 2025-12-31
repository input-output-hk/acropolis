//! Acropolis Parameter State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::{
    caryatid::{RollbackWrapper, ValidationContext},
    configuration::StartupMethod,
    declare_cardano_rdr, declare_cardano_inner,
    messages::{
        CardanoMessage, Message, ProtocolParamsMessage, StateQuery, StateQueryResponse,
        GovernanceOutcomesMessage, SnapshotMessage, SnapshotStateMessage, StateTransitionMessage
    },
    queries::{
        errors::QueryError,
        parameters::{
            ParametersStateQuery, ParametersStateQueryResponse,
            DEFAULT_PARAMETERS_QUERY_TOPIC,
        }
    },
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::{anyhow, bail, Result};
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

const CONFIG_PROTOCOL_PARAMETERS_TOPIC: (&str, &str) =
    ("publish-parameters-topic", "cardano.protocol.parameters");
const CONFIG_VALIDATION_OUTCOME_TOPIC: (&str, &str) =
    ("validation-output-topic", "cardano.validation.parameters");
const CONFIG_NETWORK_NAME: (&str, &str) = ("network-name", "mainnet");
const CONFIG_STORE_HISTORY: (&str, bool) = ("store-history", false);
/// Topic for receiving bootstrap data when starting from a CBOR dump snapshot
const CONFIG_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

declare_cardano_rdr!(GovReader, "enact-state-topic", GovernanceOutcomes, GovernanceOutcomesMessage);

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
    pub protocol_parameters_topic: String,
    pub validation_topic: String,
    pub snapshot_subscribe_topic: String,
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
            network_name: Self::conf(config, CONFIG_NETWORK_NAME),
            protocol_parameters_topic: Self::conf(config, CONFIG_PROTOCOL_PARAMETERS_TOPIC),
            parameters_query_topic: Self::conf(config, DEFAULT_PARAMETERS_QUERY_TOPIC),
            snapshot_subscribe_topic: Self::conf(config, CONFIG_SNAPSHOT_SUBSCRIBE_TOPIC),
            validation_topic: Self::conf(config, CONFIG_VALIDATION_OUTCOME_TOPIC),
            store_history: Self::conf_bool(config, CONFIG_STORE_HISTORY),
        })
    }
}

impl ParametersState {
    async fn publish_update(
        config: &Arc<ParametersStateConfig>,
        block: &BlockInfo,
        message: ProtocolParamsMessage,
    ) -> Result<()> {
        let config = config.clone();

        let packed_message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::ProtocolParams(message),
        )));

        config
            .context
            .publish(&config.protocol_parameters_topic, packed_message)
            .await?;

        Ok(())
    }

    async fn run(
        cfg: Arc<ParametersStateConfig>,
        history: Arc<Mutex<StateHistory<State>>>,
        mut gov: GovReader,
    ) -> Result<()> {
        // Process the snapshot messages first to bootstrap state if needed
        loop {
            let mut ctx = ValidationContext::new(&cfg.context, &cfg.validation_topic);
            match ctx.consume_sync(gov.read_rb().await)? {
                RollbackWrapper::Normal((block_info, gov)) => {
                    let span = info_span!("parameters_state.handle", epoch = block_info.epoch);
                    async {
                        // Get current state and current params
                        let mut state = {
                            let mut h = history.lock().await;
                            h.get_or_init_with(|| State::new(cfg.network_name.clone()))
                        };

                        // Handle rollback if needed
                        if block_info.status == BlockStatus::RolledBack {
                            state = history.lock().await.get_rolled_back_state(block_info.epoch);
                        }

                        if block_info.new_epoch {
                            // Get current params
                            let current_params = state.current_params.get_params();

                            // Process GovOutcomes message on epoch transition
                            let new_params = ctx.handle(
                                "gov enact state",
                                state.handle_enact_state(&block_info.era, &gov).await
                            );

                            // Publish protocol params message
                            ctx.handle(
                                "publish params",
                                Self::publish_update(&cfg, &block_info, new_params.clone()).await
                            );

                            // Commit state on params change
                            if current_params != new_params.params {
                                info!(
                                    "New parameter set enacted [from epoch, params]: [{},{}]",
                                    block_info.epoch,
                                    ctx.handle(
                                        "params strings",
                                        serde_json::to_string(&new_params.params)
                                            .map_err(|e| anyhow!("Serde error: {e}"))
                                    )
                                );
                                let mut h = history.lock().await;
                                h.commit(block_info.epoch, state);
                            }
                        }
                    }
                    .instrument(span)
                    .await;
                },
                RollbackWrapper::Rollback(rollback) => {
                    ctx.handle(
                        "publish rollback",
                        cfg.context.publish(&cfg.protocol_parameters_topic, rollback).await
                    );
                }
            };
            ctx.publish().await;
        }
    }

    async fn read_snapshot(
        mut subscription: Box<dyn Subscription<Message>>,
        history: Arc<Mutex<StateHistory<State>>>,
        network_name: String,
    ) {
        loop {
            let Ok((_, message)) = subscription.read().await else {
                return;
            };

            match message.as_ref() {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("ParameterState: Snapshot Startup message received");
                },
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
                },
                Message::Snapshot(SnapshotMessage::Complete) => {
                    info!("Snapshot complete, exiting Parameters state bootstrap loop");
                    break; // done processing snapshot messages
                },
                // There will be other snapshot messages that we're not interested in
                _ => (),
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = ParametersStateConfig::new(context.clone(), &config);
        let enact_s = GovReader::new_no_default(&context, &config).await?;
        let history_mode = if cfg.store_history {
            StateHistoryStore::Unbounded
        } else {
            StateHistoryStore::default_epoch_store()
        };

        // Initalize state history
        let history = StateHistory::new_mutex("ParameterState", history_mode);

        // Subscribe for snapshot messages, if booting from snapshot
        if StartupMethod::from_config(config.as_ref()).is_snapshot() {
            info!("Creating subscriber on '{}'", cfg.snapshot_subscribe_topic);
            context.run(Self::read_snapshot(
                context.subscribe(&cfg.snapshot_subscribe_topic).await?,
                history.clone(),
                cfg.network_name.clone()
            ));
        }

        let cfg_clone = cfg.clone();
        let store_history = cfg.store_history;
        let history_clone = history.clone();
        let query_state = history.clone();

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

        if let Some(enact_s) = enact_s {
            // Start run task
            tokio::spawn(async move {
                Self::run(cfg_clone, history_clone, enact_s)
                    .await
                    .unwrap_or_else(|e| error!("Failed: {e}"));
            });
        }

        Ok(())
    }
}
