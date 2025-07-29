//! Acropolis SPDD state module for Caryatid
//! Stores historical stake pool delegation distributions
use acropolis_common::{
    messages::{CardanoMessage, Message},
    rest_helper::handle_rest_with_path_parameter,
    KeyHash,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};
mod state;
use state::State;
mod rest;
use rest::handle_spdd;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.spo.distribution";
const DEFAULT_HANDLE_SPDD_TOPIC: (&str, &str) = ("handle-topic-spdd", "rest.get.spdd");

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

        let handle_spdd_topic = config
            .get_string(DEFAULT_HANDLE_SPDD_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_SPDD_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_spdd_topic);

        let state = Arc::new(Mutex::new(State::new()));

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
                            let mut state = state_handler.lock().await;

                            let spdd: BTreeMap<KeyHash, u64> =
                                msg.spos.iter().map(|(k, v)| (k.clone(), *v)).collect();

                            state.insert_spdd(msg.epoch, spdd);
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        // Register /spdd REST endpoint
        let state_rest = state.clone();
        handle_rest_with_path_parameter(context.clone(), &handle_spdd_topic, move |params| {
            let params: Vec<String> = params.iter().map(|s| s.to_string()).collect();
            handle_spdd(state_rest.clone(), params)
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

        Ok(())
    }
}
