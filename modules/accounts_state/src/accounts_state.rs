//! Acropolis accounts state module for Caryatid
//! Manages stake and reward accounts state

use caryatid_sdk::{Context, Module, module, MessageBusExt, message_bus::Subscription};
use acropolis_common::{
    messages::{Message, RESTResponse, CardanoMessage},
};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tokio::sync::Mutex;
use tracing::{error, info};
use serde_json;

mod state;
use state::State;

const DEFAULT_SPO_STATE_TOPIC: &str = "cardano.spo.state";
const DEFAULT_EPOCH_ACTIVITY_TOPIC: &str = "cardano.epoch.activity";
const DEFAULT_TX_CERTIFICATES_TOPIC: &str = "cardano.certificates";
const DEFAULT_STAKE_DELTAS_TOPIC: &str = "cardano.stake.deltas";
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.rewards";

/// Accounts State module
#[module(
    message_type(Message),
    name = "accounts-state",
    description = "Stake and reward accounts state"
)]
pub struct AccountsState;

impl AccountsState
{
    /// Async run loop
    async fn run(state: Arc<Mutex<State>>,
                 mut spos_subscription: Box<dyn Subscription<Message>>,
                 mut ea_subscription: Box<dyn Subscription<Message>>,
                 mut certs_subscription: Box<dyn Subscription<Message>>,
                 mut stake_subscription: Box<dyn Subscription<Message>>) -> Result<()> {

        // Main loop
        loop {
            // Read all topics in parallel
            let spos_message_f = spos_subscription.read();
            let ea_message_f = ea_subscription.read();
            let certs_message_f = certs_subscription.read();
            let stake_message_f = stake_subscription.read();

            // Handle SPOs
            let (_, message) = spos_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::SPOState(spo_msg))) => {
                    let mut state = state.lock().await;
                    state.handle_spo_state(block_info, spo_msg)
                        .inspect_err(|e| error!("Messaging handling error: {e}"))
                        .ok();
                }

                _ => error!("Unexpected message type: {message:?}")
            }

            // Handle epoch activity
            let (_, message) = ea_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::EpochActivity(ea_msg))) => {
                    let mut state = state.lock().await;
                    state.handle_epoch_activity(block_info, ea_msg)
                        .inspect_err(|e| error!("Messaging handling error: {e}"))
                        .ok();
                }

                _ => error!("Unexpected message type: {message:?}")
            }

            // Handle certificates
            let (_, message) = certs_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::TxCertificates(tx_certs_msg))) => {
                    let mut state = state.lock().await;
                    state.handle_tx_certificates(block_info, tx_certs_msg)
                        .inspect_err(|e| error!("Messaging handling error: {e}"))
                        .ok();
                }

                _ => error!("Unexpected message type: {message:?}")
            }

            // Handle stake address deltas
            let (_, message) = stake_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info,
                                  CardanoMessage::StakeAddressDeltas(deltas_msg))) => {
                    let mut state = state.lock().await;
                    state.handle_stake_deltas(block_info, deltas_msg)
                        .inspect_err(|e| error!("Messaging handling error: {e}"))
                        .ok();
                }

                _ => error!("Unexpected message type: {message:?}")
            }
        }
    }

    /// Async initialisation
    async fn async_init(context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Get configuration
        let spo_state_topic = config.get_string("spo-state-topic")
            .unwrap_or(DEFAULT_SPO_STATE_TOPIC.to_string());
        info!("Creating SPO state subscriber on '{spo_state_topic}'");

        let epoch_activity_topic = config.get_string("epoch-activity-topic")
            .unwrap_or(DEFAULT_EPOCH_ACTIVITY_TOPIC.to_string());
        info!("Creating epoch activity subscriber on '{epoch_activity_topic}'");

        let tx_certificates_topic = config.get_string("tx-certificates-topic")
            .unwrap_or(DEFAULT_TX_CERTIFICATES_TOPIC.to_string());
        info!("Creating Tx certificates subscriber on '{tx_certificates_topic}'");

        let stake_deltas_topic = config.get_string("stake-deltas-topic")
            .unwrap_or(DEFAULT_STAKE_DELTAS_TOPIC.to_string());
        info!("Creating stake deltas subscriber on '{stake_deltas_topic}'");

        let handle_topic = config.get_string("handle-topic")
            .unwrap_or(DEFAULT_HANDLE_TOPIC.to_string());
        info!("Creating request handler on '{handle_topic}'");

        // Create state
        let state = Arc::new(Mutex::new(State::new()));
        let state_handle_full = state.clone();
        let state_handle_single = state.clone();
        let state_tick = state.clone();

        // Handle requests for full state
        context.message_bus.handle(&handle_topic, move |message: Arc<Message>| {
            let state = state_handle_full.clone();
            async move {
                let response = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        if let Some(state) = state.lock().await.current().clone() {
                            match serde_json::to_string(state) {
                                Ok(body) => RESTResponse::with_json(200, &body),
                                Err(error) => RESTResponse::with_text(500, &format!("{error:?}").to_string()),
                            }
                        } else {
                            RESTResponse::with_json(200, "{}")
                        }
                    },
                    _ => {
                        error!("Unexpected message type {:?}", message);
                        RESTResponse::with_text(500, "Unexpected message in REST request")
                    }
                };

                Arc::new(Message::RESTResponse(response))
            }
        })?;

        let handle_topic_single = handle_topic + ".*";

        // Handle requests for single reward state based on stake address
        context.message_bus.handle(&handle_topic_single, move |message: Arc<Message>| {
            let state = state_handle_single.clone();
            async move {
                let response = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        match request.path_elements.get(1) {
                            // TODO! Stake addresses will be text encoded "stake1xxx"
                            Some(id) => match hex::decode(&id) {
                                Ok(id) => {
                                    let state = state.lock().await;
                                    match state.get_rewards(&id) {
                                        Some(reward) => match serde_json::to_string(&reward) {
                                            Ok(body) => RESTResponse::with_json(200, &body),
                                            Err(error) => RESTResponse::with_text(500, &format!("{error:?}").to_string()),
                                        },
                                        None => RESTResponse::with_text(404, "Stake address not found"),
                                    }
                                },
                                Err(error) => RESTResponse::with_text(400, &format!("Stake address must be hex encoded vector of bytes: {error:?}").to_string()),
                            },
                            None => RESTResponse::with_text(400, "Stake address must be provided"),
                        }
                    },
                    _ => {
                        error!("Unexpected message type {:?}", message);
                        RESTResponse::with_text(500, "Unexpected message in REST request")
                    }
                };

                Arc::new(Message::RESTResponse(response))
            }
        })?;

        // Ticker to log stats
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

        // Subscribe
        let spos_subscription = context.message_bus.register(&spo_state_topic).await?;
        let ea_subscription = context.message_bus.register(&epoch_activity_topic).await?;
        let certs_subscription = context.message_bus.register(&tx_certificates_topic).await?;
        let stake_subscription = context.message_bus.register(&stake_deltas_topic).await?;

        // Start run task
        tokio::spawn(async move {
            Self::run(state, spos_subscription, ea_subscription,
                      certs_subscription, stake_subscription)
                .await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }

    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        tokio::runtime::Handle::current().block_on(async move {
            Self::async_init(context, config)
                .await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
