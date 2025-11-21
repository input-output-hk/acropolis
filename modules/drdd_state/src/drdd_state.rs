//! Acropolis DRDD state module for Caryatid
//! Stores historical DRep delegation distributions
use acropolis_common::{
    messages::{CardanoMessage, Message},
    rest_helper::handle_rest_with_query_parameters,
};
use anyhow::Result;
use caryatid_sdk::{module, Context};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};
mod state;
use state::State;
mod rest;
use rest::handle_drdd;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.drep.distribution";
const DEFAULT_HANDLE_DRDD_TOPIC: (&str, &str) = ("handle-topic-drdd", "rest.get.drdd");
const DEFAULT_STORE_DRDD: (&str, bool) = ("store-drdd", false);

/// DRDD State module
#[module(
    message_type(Message),
    name = "drdd-state",
    description = "DRep Delegation Distribution State Tracker"
)]

pub struct DRDDState;

impl DRDDState {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let handle_drdd_topic = config
            .get_string(DEFAULT_HANDLE_DRDD_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_DRDD_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_drdd_topic);

        let store_drdd = config.get_bool(DEFAULT_STORE_DRDD.0).unwrap_or(DEFAULT_STORE_DRDD.1);

        let state_opt = if store_drdd {
            let state = Arc::new(Mutex::new(State::new()));

            // Subscribe for drdd messages from accounts_state
            let state_handler = state.clone();
            let mut message_subscription = context.subscribe(&subscribe_topic).await?;
            context.run(async move {
                loop {
                    let Ok((_, message)) = message_subscription.read().await else {
                        return;
                    };
                    match message.as_ref() {
                        Message::Cardano((_, CardanoMessage::DRepStakeDistribution(msg))) => {
                            let span = info_span!("drdd_state.handle", epoch = msg.epoch);
                            async {
                                let mut guard = state_handler.lock().await;

                                guard.apply_drdd_snapshot(
                                    msg.epoch,
                                    msg.drdd.dreps.iter().map(|(k, v)| (k.clone(), *v)),
                                    msg.drdd.abstain,
                                    msg.drdd.no_confidence,
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
                            let span = info_span!("drdd_state.tick", number = clock.number);
                            async {
                                state_logger
                                    .lock()
                                    .await
                                    .tick()
                                    .await
                                    .inspect_err(|e| error!("DRDD tick error: {e}"))
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

        // Register /drdd REST endpoint
        handle_rest_with_query_parameters(context.clone(), &handle_drdd_topic, move |params| {
            let state_rest = state_opt.clone();
            handle_drdd(state_rest.clone(), params)
        });

        Ok(())
    }
}
