//! Acropolis accounts state module for Caryatid
//! Manages stake and reward accounts state

use acropolis_common::{
    messages::{CardanoMessage, Message, RESTResponse},
    state_history::StateHistory,
    Address, BlockInfo, BlockStatus, StakeAddress, StakeAddressPayload,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context, MessageBusExt, Module};
use config::Config;
use serde_json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

mod drep_distribution_publisher;
use drep_distribution_publisher::DRepDistributionPublisher;
mod state;
use state::State;

const DEFAULT_SPO_STATE_TOPIC: &str = "cardano.spo.state";
const DEFAULT_EPOCH_ACTIVITY_TOPIC: &str = "cardano.epoch.activity";
const DEFAULT_TX_CERTIFICATES_TOPIC: &str = "cardano.certificates";
const DEFAULT_STAKE_DELTAS_TOPIC: &str = "cardano.stake.deltas";
const DEFAULT_DREP_STATE_TOPIC: &str = "cardano.drep.state";
const DEFAULT_DREP_DISTRIBUTION_TOPIC: &str = "cardano.drep.distribution";
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.stake";

/// Accounts State module
#[module(
    message_type(Message),
    name = "accounts-state",
    description = "Stake and reward accounts state"
)]
pub struct AccountsState;

impl AccountsState {
    /// Async run loop
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut publisher: DRepDistributionPublisher,
        mut spos_subscription: Box<dyn Subscription<Message>>,
        mut ea_subscription: Box<dyn Subscription<Message>>,
        mut certs_subscription: Box<dyn Subscription<Message>>,
        mut stake_subscription: Box<dyn Subscription<Message>>,
        mut drep_state_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        // Get the stake address deltas from the genesis bootstrap, which we know
        // don't contain any stake
        // !TODO this seems overly specific to our startup process
        let _ = stake_subscription.read().await?;

        // Main loop
        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_current_state();

            // Read per-block topics in parallel
            let certs_message_f = certs_subscription.read();
            let stake_message_f = stake_subscription.read();
            let mut new_epoch = false;
            let mut current_block: Option<BlockInfo> = None;

            // Handle certificates
            let (_, message) = certs_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::TxCertificates(tx_certs_msg))) => {
                    // Handle rollbacks on this topic only
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(&block_info);
                    }

                    state
                        .handle_tx_certificates(tx_certs_msg)
                        .inspect_err(|e| error!("Messaging handling error: {e}"))
                        .ok();
                    if block_info.new_epoch && block_info.epoch > 0 {
                        new_epoch = true;
                    }
                    current_block = Some(block_info.clone());
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Handle stake address deltas
            let (_, message) = stake_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::StakeAddressDeltas(deltas_msg))) => {
                    if let Some(ref block) = current_block {
                        if block.number != block_info.number {
                            error!(
                                expected = block.number,
                                received = block_info.number,
                                "Certificate and deltas messages re-ordered!"
                            );
                        }
                    }

                    state
                        .handle_stake_deltas(deltas_msg)
                        .inspect_err(|e| error!("Messaging handling error: {e}"))
                        .ok();
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Read from epoch-boundary messages only when it's a new epoch
            if new_epoch {
                let dreps_message_f = drep_state_subscription.read();
                let spos_message_f = spos_subscription.read();
                let ea_message_f = ea_subscription.read();

                // Handle DRep
                let (_, message) = dreps_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::DRepState(dreps_msg))) => {
                        // TODO: update our list of dreps
                        if let Err(e) = publisher
                            .publish_stake(block_info, Some(dreps_msg.dreps.clone()))
                            .await
                        {
                            tracing::error!("Error publishing drep voting stake distribution: {e}")
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }

                // Handle SPOs
                let (_, message) = spos_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::SPOState(spo_msg))) => {
                        if let Some(ref block) = current_block {
                            if block.number != block_info.number {
                                error!(
                                    expected = block.number,
                                    received = block_info.number,
                                    "Certificate and epoch SPOs messages re-ordered!"
                                );
                            }
                        }

                        state
                            .handle_spo_state(spo_msg)
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }

                // Handle epoch activity
                let (_, message) = ea_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::EpochActivity(ea_msg))) => {
                        if let Some(ref block) = current_block {
                            if block.number != block_info.number {
                                error!(
                                    expected = block.number,
                                    received = block_info.number,
                                    "Certificate and epoch activity messages re-ordered!"
                                );
                            }
                        }

                        state
                            .handle_epoch_activity(ea_msg)
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }

            // Commit the new state
            if let Some(block_info) = current_block {
                history.lock().await.commit(&block_info, state);
            }
        }
    }

    /// Async initialisation
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let spo_state_topic = config
            .get_string("spo-state-topic")
            .unwrap_or(DEFAULT_SPO_STATE_TOPIC.to_string());
        info!("Creating SPO state subscriber on '{spo_state_topic}'");

        let epoch_activity_topic = config
            .get_string("epoch-activity-topic")
            .unwrap_or(DEFAULT_EPOCH_ACTIVITY_TOPIC.to_string());
        info!("Creating epoch activity subscriber on '{epoch_activity_topic}'");

        let tx_certificates_topic = config
            .get_string("tx-certificates-topic")
            .unwrap_or(DEFAULT_TX_CERTIFICATES_TOPIC.to_string());
        info!("Creating Tx certificates subscriber on '{tx_certificates_topic}'");

        let stake_deltas_topic = config
            .get_string("stake-deltas-topic")
            .unwrap_or(DEFAULT_STAKE_DELTAS_TOPIC.to_string());
        info!("Creating stake deltas subscriber on '{stake_deltas_topic}'");

        let drep_state_topic = config
            .get_string("publish-drep-state-topic")
            .unwrap_or(DEFAULT_DREP_STATE_TOPIC.to_string());

        let drep_distribution_topic = config
            .get_string("publish-drep-distribution-topic")
            .unwrap_or(DEFAULT_DREP_DISTRIBUTION_TOPIC.to_string());

        let handle_topic = config
            .get_string("handle-topic")
            .unwrap_or(DEFAULT_HANDLE_TOPIC.to_string());
        info!("Creating request handler on '{handle_topic}'");

        // Create history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new("AccountsState")));
        let history_full = history.clone();
        let history_single = history.clone();
        let history_tick = history.clone();

        // Handle requests for full state
        context
            .message_bus
            .handle(&handle_topic, move |message: Arc<Message>| {
                let history = history_full.clone();
                async move {
                    let response = match message.as_ref() {
                        Message::RESTRequest(request) => {
                            info!("REST received {} {}", request.method, request.path);
                            if let Some(state) = history.lock().await.current().clone() {
                                match serde_json::to_string(state) {
                                    Ok(body) => RESTResponse::with_json(200, &body),
                                    Err(error) => RESTResponse::with_text(
                                        500,
                                        &format!("{error:?}").to_string(),
                                    ),
                                }
                            } else {
                                RESTResponse::with_json(200, "{}")
                            }
                        }
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
        context
            .message_bus
            .handle(&handle_topic_single, move |message: Arc<Message>| {
                let history = history_single.clone();
                async move {
                    let response = match message.as_ref() {
                        Message::RESTRequest(request) => {
                            info!("REST received {} {}", request.method, request.path);
                            match request.path_elements.get(1) {
                                Some(addr) => match Address::from_string(addr) {
                                    Ok(Address::Stake(StakeAddress {
                                        payload: StakeAddressPayload::StakeKeyHash(hash),
                                        ..
                                    })) => match history.lock().await.current() {
                                        Some(state) => match state.get_stake_state(&hash) {
                                            Some(stake) => match serde_json::to_string(&stake) {
                                                Ok(body) => RESTResponse::with_json(200, &body),
                                                Err(error) => RESTResponse::with_text(
                                                    500,
                                                    &format!("{error:?}").to_string(),
                                                ),
                                            },
                                            None => RESTResponse::with_text(
                                                404,
                                                "Stake address not found",
                                            ),
                                        },

                                        None => RESTResponse::with_text(500, "No state"),
                                    },
                                    _ => RESTResponse::with_text(400, "Not a stake address"),
                                },
                                None => {
                                    RESTResponse::with_text(400, "Stake address must be provided")
                                }
                            }
                        }
                        _ => {
                            error!("Unexpected message type {:?}", message);
                            RESTResponse::with_text(500, "Unexpected message in REST request")
                        }
                    };

                    Arc::new(Message::RESTResponse(response))
                }
            })?;

        // Ticker to log stats
        let mut tick_subscription = context.message_bus.register("clock.tick").await?;
        context.clone().run(async move {
            loop {
                let Ok((_, message)) = tick_subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        if let Some(state) = history_tick.lock().await.current() {
                            state
                                .tick()
                                .await
                                .inspect_err(|e| error!("Tick error: {e}"))
                                .ok();
                        }
                    }
                }
            }
        });

        let publisher = DRepDistributionPublisher::new(context.clone(), drep_distribution_topic);

        // Subscribe
        let spos_subscription = context.message_bus.register(&spo_state_topic).await?;
        let ea_subscription = context.message_bus.register(&epoch_activity_topic).await?;
        let certs_subscription = context.message_bus.register(&tx_certificates_topic).await?;
        let stake_subscription = context.message_bus.register(&stake_deltas_topic).await?;
        let drep_state_subscription = context.message_bus.register(&drep_state_topic).await?;

        // Start run task
        context.run(async move {
            Self::run(
                history,
                publisher,
                spos_subscription,
                ea_subscription,
                certs_subscription,
                stake_subscription,
                drep_state_subscription,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
