//! Acropolis accounts state module for Caryatid
//! Manages stake and reward accounts state

use acropolis_common::{
    messages::{CardanoMessage, Message, RESTResponse},
    rest_helper::{handle_rest, handle_rest_with_parameter},
    state_history::StateHistory,
    Address, BlockInfo, BlockStatus, Lovelace, StakeAddress, StakeAddressPayload,
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{message_bus::Subscription, module, Context, Module};
use config::Config;
use serde_json;
use std::collections::HashMap;
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
const DEFAULT_WITHDRAWALS_TOPIC: &str = "cardano.withdrawals";
const DEFAULT_STAKE_DELTAS_TOPIC: &str = "cardano.stake.deltas";
const DEFAULT_DREP_STATE_TOPIC: &str = "cardano.drep.state";
const DEFAULT_DREP_DISTRIBUTION_TOPIC: &str = "cardano.drep.distribution";

const DEFAULT_HANDLE_STAKE_TOPIC: &str = "rest.get.stake";
const DEFAULT_HANDLE_SPDD_TOPIC: &str = "rest.get.spdd";
const DEFAULT_HANDLE_POTS_TOPIC: &str = "rest.get.pots";
const DEFAULT_HANDLE_DRDD_TOPIC: &str = "rest.get.drdd";

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
        mut withdrawals_subscription: Box<dyn Subscription<Message>>,
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
            let withdrawals_message_f = withdrawals_subscription.read();
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
                        .inspect_err(|e| error!("TxCertificates handling error: {e:#}"))
                        .ok();
                    if block_info.new_epoch && block_info.epoch > 0 {
                        new_epoch = true;
                    }
                    current_block = Some(block_info.clone());
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Handle withdrawals
            let (_, message) = withdrawals_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::Withdrawals(withdrawals_msg))) => {
                    if let Some(ref block) = current_block {
                        if block.number != block_info.number {
                            error!(
                                expected = block.number,
                                received = block_info.number,
                                "Certificate and withdrawals messages re-ordered!"
                            );
                        }
                    }

                    state
                        .handle_withdrawals(withdrawals_msg)
                        .inspect_err(|e| error!("Withdrawals handling error: {e:#}"))
                        .ok();
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
                        .inspect_err(|e| error!("StakeAddressDeltas handling error: {e:#}"))
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
                        state.handle_drep_state(&dreps_msg);

                        let drdd = state.generate_drdd();

                        if let Err(e) = publisher.publish_stake(block_info, drdd).await {
                            error!("Error publishing drep voting stake distribution: {e:#}")
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
                            .inspect_err(|e| error!("SPOState handling error: {e:#}"))
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
                            .inspect_err(|e| error!("EpochActivity handling error: {e:#}"))
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

        // Subscription topics
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

        let withdrawals_topic = config
            .get_string("withdrawals-topic")
            .unwrap_or(DEFAULT_WITHDRAWALS_TOPIC.to_string());
        info!("Creating withdrawals subscriber on '{withdrawals_topic}'");

        let stake_deltas_topic = config
            .get_string("stake-deltas-topic")
            .unwrap_or(DEFAULT_STAKE_DELTAS_TOPIC.to_string());
        info!("Creating stake deltas subscriber on '{stake_deltas_topic}'");

        let drep_state_topic = config
            .get_string("drep-state-topic")
            .unwrap_or(DEFAULT_DREP_STATE_TOPIC.to_string());
        info!("Creating DRep state subscriber on '{drep_state_topic}'");

        // Publishing topics
        let drep_distribution_topic = config
            .get_string("publish-drep-distribution-topic")
            .unwrap_or(DEFAULT_DREP_DISTRIBUTION_TOPIC.to_string());

        // REST handler topics
        let handle_stake_topic = config
            .get_string("handle-stake-topic")
            .unwrap_or(DEFAULT_HANDLE_STAKE_TOPIC.to_string());
        info!("Creating request handler on '{handle_stake_topic}'");

        let handle_spdd_topic = config
            .get_string("handle-spdd-topic")
            .unwrap_or(DEFAULT_HANDLE_SPDD_TOPIC.to_string());
        info!("Creating request handler on '{handle_spdd_topic}'");

        let handle_pots_topic = config
            .get_string("handle-pots-topic")
            .unwrap_or(DEFAULT_HANDLE_POTS_TOPIC.to_string());
        info!("Creating request handler on '{handle_pots_topic}'");

        let handle_drdd_topic = config
            .get_string("handle-drdd-topic")
            .unwrap_or(DEFAULT_HANDLE_DRDD_TOPIC.to_string());
        info!("Creating request handler on '{handle_drdd_topic}'");

        // Create history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new("AccountsState")));
        let history_stake = history.clone();
        let history_stake_single = history.clone();
        let history_spdd = history.clone();
        let history_pots = history.clone();
        let history_drdd = history.clone();
        let history_tick = history.clone();

        // Handle requests for full state
        handle_rest(context.clone(), &handle_stake_topic, move || {
            let history = history_stake.clone();
            async move {
                if let Some(state) = history.lock().await.current().clone() {
                    match serde_json::to_string(state) {
                        Ok(body) => Ok(RESTResponse::with_json(200, &body)),
                        Err(error) => Err(anyhow!("{:?}", error)),
                    }
                } else {
                    Ok(RESTResponse::with_json(200, "{}"))
                }
            }
        });

        let handle_single_stake_topic = handle_stake_topic + ".*";

        // Handle requests for single reward state based on stake address
        handle_rest_with_parameter(context.clone(), &handle_single_stake_topic, move |param| {
            let history = history_stake_single.clone();
            let param = param.to_string();

            async move {
                match Address::from_string(&param) {
                    Ok(Address::Stake(StakeAddress {
                        payload: StakeAddressPayload::StakeKeyHash(hash),
                        ..
                    })) => match history.lock().await.current() {
                        Some(state) => match state.get_stake_state(&hash) {
                            Some(stake) => match serde_json::to_string(&stake) {
                                Ok(body) => Ok(RESTResponse::with_json(200, &body)),
                                Err(error) => Err(anyhow!("{:?}", error)),
                            },
                            None => Ok(RESTResponse::with_text(404, "Stake address not found")),
                        },
                        None => Err(anyhow!("No state")),
                    },
                    _ => Ok(RESTResponse::with_text(400, "Not a stake address")),
                }
            }
        });

        // Handle requests for SPDD
        handle_rest(context.clone(), &handle_spdd_topic, move || {
            let history = history_spdd.clone();
            async move {
                if let Some(state) = history.lock().await.current() {
                    // Use hex for SPO ID
                    let spdd: HashMap<String, u64> = state
                        .generate_spdd()
                        .iter()
                        .map(|(k, v)| (hex::encode(k), *v))
                        .collect();
                    match serde_json::to_string(&spdd) {
                        Ok(body) => Ok(RESTResponse::with_json(200, &body)),
                        Err(error) => Err(anyhow!("{:?}", error)),
                    }
                } else {
                    Ok(RESTResponse::with_json(200, "{}"))
                }
            }
        });

        // Handle requests for POTS
        handle_rest(context.clone(), &handle_pots_topic, move || {
            let history = history_pots.clone();
            async move {
                if let Some(state) = history.lock().await.current() {
                    let pots = state.get_pots();
                    match serde_json::to_string(&pots) {
                        Ok(body) => Ok(RESTResponse::with_json(200, &body)),
                        Err(error) => Err(anyhow!("{:?}", error)),
                    }
                } else {
                    Ok(RESTResponse::with_json(200, "{}"))
                }
            }
        });

        // Handle requests for DRDD
        handle_rest(context.clone(), &handle_drdd_topic, move || {
            let history = history_drdd.clone();
            async move {
                let drdd = history
                    .lock()
                    .await
                    .current()
                    .map(|state| state.generate_drdd())
                    .unwrap_or_default();
                let drdd = APIDRepDelegationDistribution {
                    abstain: drdd.abstain,
                    no_confidence: drdd.no_confidence,
                    dreps: drdd
                        .dreps
                        .into_iter()
                        .map(|(cred, amount)| (cred.to_json_string(), amount))
                        .collect(),
                };
                match serde_json::to_string(&drdd) {
                    Ok(body) => Ok(RESTResponse::with_json(200, &body)),
                    Err(error) => bail!("{:?}", error),
                }
            }
        });

        // Ticker to log stats
        let mut tick_subscription = context.subscribe("clock.tick").await?;
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
        let spos_subscription = context.subscribe(&spo_state_topic).await?;
        let ea_subscription = context.subscribe(&epoch_activity_topic).await?;
        let certs_subscription = context.subscribe(&tx_certificates_topic).await?;
        let withdrawals_subscription = context.subscribe(&withdrawals_topic).await?;
        let stake_subscription = context.subscribe(&stake_deltas_topic).await?;
        let drep_state_subscription = context.subscribe(&drep_state_topic).await?;

        // Start run task
        context.run(async move {
            Self::run(
                history,
                publisher,
                spos_subscription,
                ea_subscription,
                certs_subscription,
                withdrawals_subscription,
                stake_subscription,
                drep_state_subscription,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct APIDRepDelegationDistribution {
    pub abstain: Lovelace,
    pub no_confidence: Lovelace,
    pub dreps: Vec<(String, u64)>,
}
