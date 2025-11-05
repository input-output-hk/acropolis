//! Acropolis SPDD state module for Caryatid
//! Stores historical stake pool delegation distributions
use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::spdd::{SPDDStateQuery, SPDDStateQueryResponse, DEFAULT_SPDD_QUERY_TOPIC},
    rest_helper::handle_rest_with_query_parameters,
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
use rest::handle_spdd;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.spo.distribution";
const DEFAULT_HANDLE_SPDD_TOPIC: (&str, &str) = ("handle-topic-spdd", "rest.get.spdd");
const DEFAULT_STORE_SPDD: (&str, bool) = ("store-spdd", false);

/// SPDD State module
#[module(
    message_type(Message),
    name = "spdd-state",
    description = "Stake Pool Delegation Distribution State Tracker"
)]

pub struct SPDDState;

impl SPDDState {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        // REST topic (not included in BF)
        let handle_spdd_topic = config
            .get_string(DEFAULT_HANDLE_SPDD_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_SPDD_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_spdd_topic);

        // Query topic
        let spdd_query_topic = config
            .get_string(DEFAULT_SPDD_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_SPDD_QUERY_TOPIC.1.to_string());
        info!("Creating query handler on '{}'", spdd_query_topic);

        let store_spdd = config.get_bool(DEFAULT_STORE_SPDD.0).unwrap_or(DEFAULT_STORE_SPDD.1);

        let state_opt = if store_spdd {
            let state = Arc::new(Mutex::new(State::new()));

            // Register /spdd REST endpoint
            let state_rest = state.clone();
            handle_rest_with_query_parameters(context.clone(), &handle_spdd_topic, move |params| {
                handle_spdd(state_rest.clone(), params)
            });

            // Subscribe for spdd messages from accounts_state
            let state_handler = state.clone();
            let mut message_subscription = context.subscribe(&subscribe_topic).await?;
            context.run(async move {
                loop {
                    let Ok((_, message)) = message_subscription.read().await else {
                        return;
                    };
                    match message.as_ref() {
                        Message::Cardano((_, CardanoMessage::SPOStakeDistribution(msg))) => {
                            let span = info_span!("spdd_state.handle", epoch = msg.epoch);
                            async {
                                let mut guard = state_handler.lock().await;

                                guard.apply_spdd_snapshot(
                                    msg.epoch,
                                    msg.spos.iter().map(|(k, v)| (*k, *v)),
                                );
                            }
                            .instrument(span)
                            .await;
                        }

                        _ => error!("Unexpected message type: {message:?}"),
                    }
                }
            });
            // Ticker to log stats
            let mut tick_subscription = context.subscribe("clock.tick").await?;
            let state_logger = state.clone();
            context.run(async move {
                loop {
                    let Ok((_, message)) = tick_subscription.read().await else {
                        return;
                    };

                    if let Message::Clock(clock) = message.as_ref() {
                        if clock.number % 60 == 0 {
                            let span = info_span!("spdd_state.tick", number = clock.number);
                            async {
                                state_logger
                                    .lock()
                                    .await
                                    .tick()
                                    .await
                                    .inspect_err(|e| error!("SPDD tick error: {e}"))
                                    .ok();
                            }
                            .instrument(span)
                            .await;
                        }
                    }
                }
            });
            Some(state)
        } else {
            None
        };

        // handle spdd query
        context.handle(&spdd_query_topic, move |message| {
            let state = state_opt.clone();
            async move {
                let Message::StateQuery(StateQuery::SPDD(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::SPDD(
                        SPDDStateQueryResponse::Error("Invalid message for spdd-state".into()),
                    )));
                };
                let Some(state) = state else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::SPDD(
                        SPDDStateQueryResponse::Error("SPDD storage is NOT enabled".into()),
                    )));
                };
                let state = state.lock().await;

                let response = match query {
                    SPDDStateQuery::GetEpochTotalActiveStakes { epoch } => {
                        SPDDStateQueryResponse::EpochTotalActiveStakes(
                            state.get_epoch_total_active_stakes(*epoch).unwrap_or(0),
                        )
                    }
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::SPDD(
                    response,
                )))
            }
        });

        Ok(())
    }
}
